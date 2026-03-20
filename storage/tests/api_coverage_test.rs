use axiom_crypto::compute_block_hash;
use axiom_primitives::{AccountId, Block, BlockHash, StateHash, ValidatorId};
use axiom_state::{Account, State, Validator};
use axiom_storage::Storage;
use std::collections::BTreeMap;

// Helpers
fn dummy_hash(b: u8) -> BlockHash {
    BlockHash([b; 32])
}
fn dummy_state_hash(b: u8) -> StateHash {
    StateHash([b; 32])
}
fn dummy_account_id(b: u8) -> AccountId {
    AccountId([b; 32])
}
fn dummy_validator_id(b: u8) -> ValidatorId {
    ValidatorId([b; 32])
}

fn create_dummy_state() -> State {
    let mut accounts = BTreeMap::new();
    accounts.insert(
        dummy_account_id(1),
        Account {
            balance: 1000,
            nonce: 0,
        },
    );
    accounts.insert(
        dummy_account_id(2),
        Account {
            balance: 500,
            nonce: 1,
        },
    );

    let mut validators = BTreeMap::new();
    validators.insert(
        dummy_validator_id(1),
        Validator {
            voting_power: 10,
            account_id: dummy_account_id(1),
            active: true,
        },
    );

    State {
        accounts,
        validators,
        total_supply: 1500,
        block_reward: 10,
    }
}

fn create_dummy_block(height: u64, parent: BlockHash) -> Block {
    Block {
        parent_hash: parent,
        height,
        epoch: 1,
        protocol_version: axiom_primitives::PROTOCOL_VERSION_V1,
        round: 0,
        proposer_id: dummy_validator_id(1),
        transactions: vec![],
        signatures: vec![],
        state_hash: dummy_state_hash(height as u8),
        timestamp: 0,
    }
}

#[test]
fn test_genesis_store_retrieve() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_genesis.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let state = create_dummy_state();
    let genesis_hash = dummy_state_hash(0xAA);

    storage.store_genesis(&state, &genesis_hash).unwrap();

    // Verify Genesis Hash
    let stored_hash = storage.get_genesis_hash().unwrap();
    assert_eq!(stored_hash, genesis_hash);

    // Verify State Reconstruction
    let loaded_state = storage.load_latest_state().unwrap().unwrap().0;
    assert_eq!(loaded_state.total_supply, state.total_supply);
    assert_eq!(loaded_state.block_reward, state.block_reward);
    assert_eq!(loaded_state.accounts.len(), 2);
    assert_eq!(loaded_state.validators.len(), 1);
}

#[test]
fn test_block_lookups_round_trip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_blocks.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let state = create_dummy_state();
    let genesis_hash = dummy_state_hash(0x00);
    storage.store_genesis(&state, &genesis_hash).unwrap();

    let block1 = create_dummy_block(1, dummy_hash(0x00));
    let block1_hash = compute_block_hash(&block1);

    storage.commit_block(&block1, &state).unwrap();

    // 1. Lookup by Height
    let retrieved_by_height = storage
        .get_block_by_height(1)
        .unwrap()
        .expect("Block 1 should exist");
    assert_eq!(retrieved_by_height, block1);

    // 2. Lookup by Hash
    let retrieved_by_hash = storage
        .get_block_by_hash(&block1_hash)
        .unwrap()
        .expect("Block 1 should exist by hash");
    assert_eq!(retrieved_by_hash, block1);
}

#[test]
fn test_non_existent_lookups() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_empty.db");
    let storage = Storage::initialize(&db_path).unwrap();

    // Block lookups
    assert!(storage.get_block_by_height(999).unwrap().is_none());
    assert!(storage
        .get_block_by_hash(&dummy_hash(0xFF))
        .unwrap()
        .is_none());

    // Account lookup
    assert!(storage
        .get_account(&dummy_account_id(0x99))
        .unwrap()
        .is_none());
}

#[test]
fn test_account_retrieval_after_change() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_accounts.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let mut state = create_dummy_state();
    let genesis_hash = dummy_state_hash(0x00);
    storage.store_genesis(&state, &genesis_hash).unwrap();

    // Verify initial state
    let acc1 = storage
        .get_account(&dummy_account_id(1))
        .unwrap()
        .expect("Account 1 should exist");
    assert_eq!(acc1.balance, 1000);

    // Modify state (simulate block execution)
    state.accounts.insert(
        dummy_account_id(1),
        Account {
            balance: 2000,
            nonce: 1,
        },
    );
    let block1 = create_dummy_block(1, dummy_hash(0x00));

    storage.commit_block(&block1, &state).unwrap();

    // Verify updated state
    let acc1_updated = storage
        .get_account(&dummy_account_id(1))
        .unwrap()
        .expect("Account 1 should exist");
    assert_eq!(acc1_updated.balance, 2000);
    assert_eq!(acc1_updated.nonce, 1);
}

#[test]
fn test_latest_height_tracking() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_height.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let state = create_dummy_state();
    storage
        .store_genesis(&state, &dummy_state_hash(0x00))
        .unwrap();

    assert_eq!(storage.get_latest_height().unwrap(), 0);

    let block1 = create_dummy_block(1, dummy_hash(0x00));
    storage.commit_block(&block1, &state).unwrap();
    assert_eq!(storage.get_latest_height().unwrap(), 1);

    let block2 = create_dummy_block(2, compute_block_hash(&block1));
    storage.commit_block(&block2, &state).unwrap();
    assert_eq!(storage.get_latest_height().unwrap(), 2);
}

#[test]
fn test_validator_retrieval() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_validators.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let state = create_dummy_state();
    storage
        .store_genesis(&state, &dummy_state_hash(0x00))
        .unwrap();

    let validators = storage.get_validators().unwrap();
    assert_eq!(validators.len(), 1);
    assert_eq!(validators[0].0, dummy_validator_id(1));
    assert_eq!(validators[0].1.voting_power, 10);
}

#[test]
fn test_atomic_safety_sanity() {
    // This test verifies that we can perform multiple commits and the state remains consistent.
    // True atomicity (crash recovery) is handled by SQLite WAL, which we can't easily test without killing the process.
    // But we can verify that `commit_block` updates both block info and state info together from the API perspective.

    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_atomic.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let mut state = create_dummy_state();
    storage
        .store_genesis(&state, &dummy_state_hash(0x00))
        .unwrap();

    let block1 = create_dummy_block(1, dummy_hash(0x00));

    // Mutate state for the block
    state.total_supply += 100;

    storage.commit_block(&block1, &state).unwrap();

    // Verify Block is there
    assert!(storage.get_block_by_height(1).unwrap().is_some());

    // Verify State is updated
    let loaded_state = storage.load_latest_state().unwrap().unwrap().0;
    assert_eq!(loaded_state.total_supply, 1600);
}
