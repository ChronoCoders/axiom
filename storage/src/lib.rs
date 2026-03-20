use axiom_crypto::compute_block_hash;
use axiom_primitives::{
    AccountId, Block, BlockHash, LockState, ProtocolVersion, StakeAmount, StateHash, UnbondingEntry,
    ValidatorId,
};
use axiom_state::{Account, StakingState, State, Validator};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Storage Errors
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Block not found: height {0}")]
    BlockNotFound(u64),
    #[error("State not found")]
    StateNotFound,
    #[error("Initialization error: {0}")]
    Initialization(String),
    #[error("Data corruption: {0}")]
    Corruption(String),
    #[error("Internal lock poisoned")]
    LockPoisoned,
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage interface for SQLite
/// Implements Phase 7 requirements from IMPLEMENTATION_GUIDE.md
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    /// Initialize storage at the given path
    /// WAL mode enabled.
    pub fn initialize<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let conn = Connection::open(path_ref)?;

        // Enable WAL mode for better concurrency/safety
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;

        // Table: blocks
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
                height INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                parent_hash TEXT NOT NULL,
                epoch INTEGER NOT NULL,
                proposer_id TEXT NOT NULL,
                state_hash TEXT NOT NULL,
                block_data BLOB NOT NULL,
                timestamp INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;

        // Table: accounts
        conn.execute(
            "CREATE TABLE IF NOT EXISTS accounts (
                account_id TEXT PRIMARY KEY,
                balance INTEGER NOT NULL,
                nonce INTEGER NOT NULL
            )",
            [],
        )?;

        // Table: validators
        conn.execute(
            "CREATE TABLE IF NOT EXISTS validators (
                validator_id TEXT PRIMARY KEY,
                voting_power INTEGER NOT NULL,
                account_id TEXT NOT NULL,
                active INTEGER NOT NULL
            )",
            [],
        )?;

        // Table: meta
        conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        // Table: pending_blocks (Durability Requirement 1 & 5)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS pending_blocks (
                height INTEGER NOT NULL,
                hash TEXT NOT NULL PRIMARY KEY,
                block_data BLOB NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )?;

        // Table: self_votes (Durability Requirement 2)
        // height is PRIMARY KEY to enforce single vote per height at DB level
        conn.execute(
            "CREATE TABLE IF NOT EXISTS self_votes (
                height INTEGER PRIMARY KEY,
                block_hash TEXT NOT NULL,
                signature TEXT NOT NULL
            )",
            [],
        )?;

        // v2 schema tables (scaffolding — remain empty during v1)

        // Table: stakes (v2 staking state)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS stakes (
                validator_id TEXT PRIMARY KEY,
                amount INTEGER NOT NULL
            )",
            [],
        )?;

        // Table: unbonding_queue (v2 unbonding entries)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS unbonding_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                validator_id TEXT NOT NULL,
                amount INTEGER NOT NULL,
                release_height INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS consensus_locks (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                height INTEGER NOT NULL,
                round INTEGER NOT NULL,
                block_hash TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS votes (
                height INTEGER NOT NULL,
                round INTEGER NOT NULL,
                phase INTEGER NOT NULL,
                validator_id TEXT NOT NULL,
                block_hash TEXT,
                signature TEXT NOT NULL,
                PRIMARY KEY (height, round, phase, validator_id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                height INTEGER NOT NULL,
                round INTEGER NOT NULL,
                validator_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload BLOB NOT NULL
            )",
            [],
        )?;

        // Table: schema_version (tracks DB schema version for migrations)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY
            )",
            [],
        )?;

        // Insert schema version 1 if not present
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version) VALUES (1)",
            [],
        )?;

        // Migration: v1 -> v2 (add timestamp column to blocks)
        let current_version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap_or(1);

        if current_version < 2 {
            let has_timestamp: bool = conn
                .prepare("SELECT timestamp FROM blocks LIMIT 0")
                .is_ok();
            if !has_timestamp {
                conn.execute(
                    "ALTER TABLE blocks ADD COLUMN timestamp INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
            }
            conn.execute(
                "INSERT OR IGNORE INTO schema_version (version) VALUES (2)",
                [],
            )?;
        }

        Ok(())
    }

    /// Store genesis state and hash
    pub fn store_genesis(&self, state: &State, genesis_hash: &StateHash) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let tx = conn.transaction()?;

        // Store accounts
        for (id, account) in &state.accounts {
            tx.execute(
                "INSERT OR REPLACE INTO accounts (account_id, balance, nonce) VALUES (?1, ?2, ?3)",
                params![id.to_string(), account.balance, account.nonce],
            )?;
        }

        // Store validators
        for (id, validator) in &state.validators {
            tx.execute(
                "INSERT OR REPLACE INTO validators (validator_id, voting_power, account_id, active) VALUES (?1, ?2, ?3, ?4)",
                params![id.to_string(), validator.voting_power, validator.account_id.to_string(), validator.active as i32],
            )?;
        }

        // Store meta
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('genesis_hash', ?1)",
            params![genesis_hash.to_string()],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('total_supply', ?1)",
            params![state.total_supply.to_string()],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('block_reward', ?1)",
            params![state.block_reward.to_string()],
        )?;

        // Initial height is 0 (before first block)
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('latest_height', '0')",
            [],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Atomically commit a block and update state
    pub fn commit_block(&self, block: &Block, state: &State) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let tx = conn.transaction()?;

        // 1. Store Block
        let block_hash = compute_block_hash(block);
        let block_data = serde_json::to_vec(block)?;

        tx.execute(
            "INSERT INTO blocks (height, hash, parent_hash, epoch, proposer_id, state_hash, block_data, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                block.height,
                block_hash.to_string(),
                block.parent_hash.to_string(),
                block.epoch,
                block.proposer_id.to_string(),
                block.state_hash.to_string(),
                block_data,
                block.timestamp
            ],
        )?;

        // 2. Update Accounts (Upsert all for simplicity/correctness with minimal logic)
        // In a real optimized system, we'd only update modified accounts.
        // Here we just iterate the state map.
        // First, clear existing? No, that's too slow. Upsert is better.
        // But what if an account is removed? (Not possible in V1, no delete).
        for (id, account) in &state.accounts {
            tx.execute(
                "INSERT OR REPLACE INTO accounts (account_id, balance, nonce) VALUES (?1, ?2, ?3)",
                params![id.to_string(), account.balance, account.nonce],
            )?;
        }

        // 3. Update Validators
        for (id, validator) in &state.validators {
            tx.execute(
                "INSERT OR REPLACE INTO validators (validator_id, voting_power, account_id, active) VALUES (?1, ?2, ?3, ?4)",
                params![id.to_string(), validator.voting_power, validator.account_id.to_string(), validator.active as i32],
            )?;
        }

        // 4. Update Meta
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('latest_height', ?1)",
            params![block.height],
        )?;

        // Also update supply/reward if they changed (unlikely in V1 but safe)
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('total_supply', ?1)",
            params![state.total_supply.to_string()],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('block_reward', ?1)",
            params![state.block_reward.to_string()],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Get block by height
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT block_data FROM blocks WHERE height = ?1")?;
        let mut rows = stmt.query(params![height])?;

        if let Some(row) = rows.next()? {
            let data: Vec<u8> = row.get(0)?;
            // Decode block
            let block: Block = serde_json::from_slice(&data)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT block_data FROM blocks WHERE hash = ?1")?;
        let mut rows = stmt.query(params![hash.to_string()])?;

        if let Some(row) = rows.next()? {
            let data: Vec<u8> = row.get(0)?;
            let block: Block = serde_json::from_slice(&data)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    /// Get latest height
    pub fn get_latest_height(&self) -> Result<u64> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = 'latest_height'")?;
        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let val: String = row.get(0)?;
            Ok(val.parse().unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    /// Get account by ID
    pub fn get_account(&self, id: &AccountId) -> Result<Option<Account>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT balance, nonce FROM accounts WHERE account_id = ?1")?;
        let mut rows = stmt.query(params![id.to_string()])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Account {
                balance: row.get(0)?,
                nonce: row.get(1)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get all validators
    pub fn get_validators(&self) -> Result<Vec<(ValidatorId, Validator)>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt =
            conn.prepare("SELECT validator_id, voting_power, account_id, active FROM validators")?;
        let mut rows = stmt.query([])?;

        let mut validators = Vec::new();
        while let Some(row) = rows.next()? {
            let vid_str: String = row.get(0)?;
            // Need to parse hex string to ValidatorId
            // Assuming we have a helper or can do it manually.
            // ValidatorId is [u8; 32]
            let vid_bytes = hex::decode(&vid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid validator id: {e}")))?;
            if vid_bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid validator id length".to_string(),
                ));
            }
            let mut vid_arr = [0u8; 32];
            vid_arr.copy_from_slice(&vid_bytes);
            let vid = ValidatorId(vid_arr);

            let aid_str: String = row.get(2)?;
            let aid_bytes = hex::decode(&aid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid account id: {e}")))?;
            if aid_bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid account id length".to_string(),
                ));
            }
            let mut aid_arr = [0u8; 32];
            aid_arr.copy_from_slice(&aid_bytes);
            let aid = AccountId(aid_arr);

            let voting_power: u64 = row.get(1)?;
            let active_int: i32 = row.get(3)?;

            validators.push((
                vid,
                Validator {
                    voting_power,
                    account_id: aid,
                    active: active_int != 0,
                },
            ));
        }
        Ok(validators)
    }

    pub fn get_validator(&self, id: &ValidatorId) -> Result<Option<Validator>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT voting_power, account_id, active FROM validators WHERE validator_id = ?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;

        if let Some(row) = rows.next()? {
            let aid_str: String = row.get(1)?;
            let aid_bytes = hex::decode(&aid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid account id: {e}")))?;
            if aid_bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid account id length".to_string(),
                ));
            }
            let mut aid_arr = [0u8; 32];
            aid_arr.copy_from_slice(&aid_bytes);
            let aid = AccountId(aid_arr);

            let voting_power: u64 = row.get(0)?;
            let active_int: i32 = row.get(2)?;

            Ok(Some(Validator {
                voting_power,
                account_id: aid,
                active: active_int != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get genesis hash
    pub fn get_genesis_hash(&self) -> Result<StateHash> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = 'genesis_hash'")?;

        if stmt.exists([])? {
            let mut rows = stmt.query([])?;
            let row = rows.next()?.ok_or(StorageError::StateNotFound)?;
            let val: String = row.get(0)?;
            let bytes = hex::decode(&val)
                .map_err(|e| StorageError::Corruption(format!("Invalid genesis hash: {e}")))?;
            if bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid genesis hash length".to_string(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Ok(StateHash(arr))
        } else {
            Err(StorageError::StateNotFound)
        }
    }

    /// Load the latest state from the database
    pub fn load_latest_state(&self) -> Result<Option<(State, u64)>> {
        let height = self.get_latest_height()?;
        // If height is 0, we might still have genesis state.
        // Check if genesis_hash exists in meta.
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;

        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = 'genesis_hash'")?;
        if !stmt.exists([])? {
            return Ok(None);
        }

        // Read Meta
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = 'total_supply'")?;
        let total_supply: u64 = stmt.query_row([], |row| {
            let s: String = row.get(0)?;
            Ok(s.parse().unwrap_or(0))
        })?;

        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = 'block_reward'")?;
        let block_reward: u64 = stmt.query_row([], |row| {
            let s: String = row.get(0)?;
            Ok(s.parse().unwrap_or(0))
        })?;

        // Read Accounts
        let mut accounts = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT account_id, balance, nonce FROM accounts")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let id_bytes = hex::decode(&id_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid account id: {e}")))?;
            let mut id_arr = [0u8; 32];
            id_arr.copy_from_slice(&id_bytes);
            let id = AccountId(id_arr);

            accounts.insert(
                id,
                Account {
                    balance: row.get(1)?,
                    nonce: row.get(2)?,
                },
            );
        }

        // Read Validators
        let mut validators = BTreeMap::new();
        let mut stmt =
            conn.prepare("SELECT validator_id, voting_power, account_id, active FROM validators")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let vid_str: String = row.get(0)?;
            let vid_bytes = hex::decode(&vid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid validator id: {e}")))?;
            let mut vid_arr = [0u8; 32];
            vid_arr.copy_from_slice(&vid_bytes);
            let vid = ValidatorId(vid_arr);

            let aid_str: String = row.get(2)?;
            let aid_bytes = hex::decode(&aid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid account id: {e}")))?;
            let mut aid_arr = [0u8; 32];
            aid_arr.copy_from_slice(&aid_bytes);
            let aid = AccountId(aid_arr);

            validators.insert(
                vid,
                Validator {
                    voting_power: row.get(1)?,
                    account_id: aid,
                    active: row.get::<_, i32>(3)? != 0,
                },
            );
        }

        Ok(Some((
            State {
                total_supply,
                block_reward,
                accounts,
                validators,
            },
            height,
        )))
    }

    /// Save a pending block (Proposal or Future block)
    /// Durability Requirement 1: Pending Block Persistence
    pub fn save_pending_block(&self, block: &Block) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let block_hash = compute_block_hash(block);
        let block_data = serde_json::to_vec(block)?;

        tx.execute(
            "INSERT OR REPLACE INTO pending_blocks (height, hash, block_data, is_active)
             VALUES (?1, ?2, ?3, 1)",
            params![block.height, block_hash.to_string(), block_data],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Get active pending blocks by height
    pub fn get_pending_blocks_by_height(&self, height: u64) -> Result<Vec<Block>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn
            .prepare("SELECT block_data FROM pending_blocks WHERE height = ?1 AND is_active = 1")?;
        let mut rows = stmt.query(params![height])?;

        let mut blocks = Vec::new();
        while let Some(row) = rows.next()? {
            let data: Vec<u8> = row.get(0)?;
            let block: Block = serde_json::from_slice(&data)?;
            blocks.push(block);
        }
        Ok(blocks)
    }

    /// Mark pending blocks as inactive up to a certain height
    /// Durability Requirement 4: Safe Cleanup
    pub fn mark_pending_blocks_inactive(&self, height: u64) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        // Mark blocks at this height or lower as inactive
        conn.execute(
            "UPDATE pending_blocks SET is_active = 0 WHERE height <= ?1",
            params![height],
        )?;
        Ok(())
    }

    /// Save own vote to prevent double signing
    /// Durability Requirement 2: Vote Persistence
    pub fn save_own_vote(
        &self,
        height: u64,
        block_hash: &BlockHash,
        signature: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        // This will fail if a vote already exists for this height (PRIMARY KEY constraint)
        conn.execute(
            "INSERT INTO self_votes (height, block_hash, signature) VALUES (?1, ?2, ?3)",
            params![height, block_hash.to_string(), signature],
        )?;
        Ok(())
    }

    /// Get own vote at height
    pub fn get_own_vote(&self, height: u64) -> Result<Option<(BlockHash, String)>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt =
            conn.prepare("SELECT block_hash, signature FROM self_votes WHERE height = ?1")?;
        let mut rows = stmt.query(params![height])?;

        if let Some(row) = rows.next()? {
            let hash_str: String = row.get(0)?;
            let bytes = hex::decode(&hash_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid block hash: {e}")))?;
            if bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid block hash length".to_string(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let signature: String = row.get(1)?;
            Ok(Some((BlockHash(arr), signature)))
        } else {
            Ok(None)
        }
    }

    // -------------------------------------------------------------------------
    // v2 Staking Storage
    // -------------------------------------------------------------------------

    /// Atomically commit a block and update state, including staking state for v2 blocks.
    pub fn commit_block_v2(
        &self,
        block: &Block,
        state: &State,
        staking: &StakingState,
    ) -> Result<()> {
        let version = ProtocolVersion::for_height(block.height);

        let mut conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let tx = conn.transaction()?;

        let block_hash = compute_block_hash(block);
        let block_data = serde_json::to_vec(block)?;

        tx.execute(
            "INSERT INTO blocks (height, hash, parent_hash, epoch, proposer_id, state_hash, block_data, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                block.height,
                block_hash.to_string(),
                block.parent_hash.to_string(),
                block.epoch,
                block.proposer_id.to_string(),
                block.state_hash.to_string(),
                block_data,
                block.timestamp
            ],
        )?;

        for (id, account) in &state.accounts {
            tx.execute(
                "INSERT OR REPLACE INTO accounts (account_id, balance, nonce) VALUES (?1, ?2, ?3)",
                params![id.to_string(), account.balance, account.nonce],
            )?;
        }

        for (id, validator) in &state.validators {
            tx.execute(
                "INSERT OR REPLACE INTO validators (validator_id, voting_power, account_id, active) VALUES (?1, ?2, ?3, ?4)",
                params![id.to_string(), validator.voting_power, validator.account_id.to_string(), validator.active as i32],
            )?;
        }

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('latest_height', ?1)",
            params![block.height],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('total_supply', ?1)",
            params![state.total_supply.to_string()],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('block_reward', ?1)",
            params![state.block_reward.to_string()],
        )?;

        if matches!(version, ProtocolVersion::V2) {
            tx.execute("DELETE FROM stakes", [])?;
            for (vid, amount) in &staking.stakes {
                tx.execute(
                    "INSERT INTO stakes (validator_id, amount) VALUES (?1, ?2)",
                    params![vid.to_string(), amount.0],
                )?;
            }

            tx.execute("DELETE FROM unbonding_queue", [])?;
            for entry in &staking.unbonding_queue {
                tx.execute(
                    "INSERT INTO unbonding_queue (validator_id, amount, release_height) VALUES (?1, ?2, ?3)",
                    params![entry.validator_id.to_string(), entry.amount.0, entry.release_height],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Load staking state from the database.
    /// Returns an empty StakingState if no staking data exists (v1 blocks).
    pub fn load_staking_state(&self) -> Result<StakingState> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;

        let mut stakes = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT validator_id, amount FROM stakes ORDER BY validator_id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let vid_str: String = row.get(0)?;
            let vid_bytes = hex::decode(&vid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid validator id in stakes: {e}")))?;
            if vid_bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid validator id length in stakes".to_string(),
                ));
            }
            let mut vid_arr = [0u8; 32];
            vid_arr.copy_from_slice(&vid_bytes);
            let vid = ValidatorId(vid_arr);
            let amount: u64 = row.get(1)?;
            stakes.insert(vid, StakeAmount(amount));
        }

        let mut unbonding_queue = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT validator_id, amount, release_height FROM unbonding_queue ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let vid_str: String = row.get(0)?;
            let vid_bytes = hex::decode(&vid_str)
                .map_err(|e| StorageError::Corruption(format!("Invalid validator id in unbonding: {e}")))?;
            if vid_bytes.len() != 32 {
                return Err(StorageError::Corruption(
                    "Invalid validator id length in unbonding".to_string(),
                ));
            }
            let mut vid_arr = [0u8; 32];
            vid_arr.copy_from_slice(&vid_bytes);
            let vid = ValidatorId(vid_arr);
            let amount: u64 = row.get(1)?;
            let release_height: u64 = row.get(2)?;
            unbonding_queue.push(UnbondingEntry {
                validator_id: vid,
                amount: StakeAmount(amount),
                release_height,
            });
        }

        if stakes.is_empty() && unbonding_queue.is_empty() {
            Ok(StakingState::empty())
        } else {
            Ok(StakingState {
                stakes,
                minimum_stake: axiom_primitives::MIN_VALIDATOR_STAKE,
                unbonding_period: axiom_primitives::UNBONDING_PERIOD,
                unbonding_queue,
            })
        }
    }

    pub fn save_lock_state(&self, lock: &LockState) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let block_hash_str = lock.block_hash.map(|h| h.to_string());
        conn.execute(
            "INSERT INTO consensus_locks (id, height, round, block_hash)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET height = excluded.height, round = excluded.round, block_hash = excluded.block_hash",
            params![lock.height, lock.round, block_hash_str],
        )?;
        Ok(())
    }

    pub fn load_lock_state(&self) -> Result<Option<LockState>> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;

        let row = conn.query_row(
            "SELECT height, round, block_hash FROM consensus_locks WHERE id = 1",
            [],
            |row| {
                let height: u64 = row.get(0)?;
                let round: u64 = row.get(1)?;
                let block_hash_str: Option<String> = row.get(2)?;
                Ok((height, round, block_hash_str))
            },
        );

        match row {
            Ok((height, round, block_hash_str)) => {
                let block_hash = match block_hash_str {
                    Some(s) => {
                        let bytes = hex::decode(&s).map_err(|e| {
                            StorageError::Corruption(format!("Invalid block hash in consensus_locks: {e}"))
                        })?;
                        if bytes.len() != 32 {
                            return Err(StorageError::Corruption(
                                "Invalid block hash length in consensus_locks".to_string(),
                            ));
                        }
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&bytes);
                        Some(BlockHash(arr))
                    }
                    None => None,
                };
                Ok(Some(LockState {
                    height,
                    round,
                    block_hash,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
