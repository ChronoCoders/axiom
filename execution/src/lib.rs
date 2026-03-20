use axiom_crypto::{
    compute_block_hash, sha256, verify_transaction_signature, verify_vote, CryptoError,
};
use axiom_primitives::{
    serialize_block_canonical, AccountId, Block, BlockHash, ProtocolVersion, StakeAmount, StateHash,
    Transaction, TransactionType, ValidatorId, MIN_VALIDATOR_STAKE, MAX_BLOCK_SIZE_BYTES,
    MAX_TRANSACTIONS_PER_BLOCK,
};
use axiom_state::{verify_staking_invariants, Account, StakingState, State, StateError};
use std::collections::HashSet;
use thiserror::Error;

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Invalid height: expected {expected}, got {got}")]
    InvalidHeight { expected: u64, got: u64 },

    #[error("Invalid parent hash: expected {expected}, got {got}")]
    InvalidParentHash { expected: BlockHash, got: BlockHash },

    #[error("Invalid epoch: expected {expected}, got {got}")]
    InvalidEpoch { expected: u64, got: u64 },

    #[error("Invalid proposer: expected {expected}, got {got}")]
    InvalidProposer {
        expected: ValidatorId,
        got: ValidatorId,
    },

    #[error("Block contains too many transactions: {count} > {max}")]
    BlockTooManyTransactions { count: usize, max: usize },

    #[error("Block too large: {size} bytes > {max} bytes")]
    BlockTooLarge { size: usize, max: usize },

    #[error("Quorum not met: collected {collected}, required > {required}")]
    QuorumNotMet { collected: u64, required: u64 },

    #[error("Duplicate signature from validator {validator}")]
    DuplicateSignature { validator: ValidatorId },

    #[error("Unknown validator {id}")]
    UnknownValidator { id: ValidatorId },

    #[error("Inactive validator {id}")]
    InactiveValidator { id: ValidatorId },

    #[error("No active validators")]
    NoActiveValidators,

    #[error("Invalid transaction signature from sender {sender}")]
    InvalidSignature { sender: AccountId },

    #[error("Sender not found: {sender}")]
    SenderNotFound { sender: AccountId },

    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Insufficient balance for {account}: required {required}, available {available}")]
    InsufficientBalance {
        account: AccountId,
        required: u64,
        available: u64,
    },

    #[error("Transaction amount must be positive")]
    ZeroAmount,

    #[error("Arithmetic overflow")]
    Overflow,

    #[error("Arithmetic underflow")]
    Underflow,

    #[error("State hash mismatch: expected {expected}, computed {computed}")]
    StateHashMismatch {
        expected: StateHash,
        computed: StateHash,
    },

    #[error("Transaction type {tx_type} not allowed at height {height} (v1 only supports Transfer)")]
    V2TransactionInV1Block {
        tx_type: TransactionType,
        height: u64,
    },

    #[error("SlashEvidence transactions are not implemented in Phase 2")]
    SlashEvidenceNotImplemented,

    #[error("Sender {sender} is not a registered validator account")]
    NotValidatorAccount { sender: AccountId },

    #[error("Stake amount {amount} below minimum {minimum}")]
    StakeBelowMinimum { amount: u64, minimum: u64 },

    #[error("Account {account} already has an active stake")]
    AlreadyStaked { account: AccountId },

    #[error("No active stake for account {account}")]
    NoActiveStake { account: AccountId },

    #[error("Unstake amount {requested} exceeds staked amount {available}")]
    InsufficientStake { requested: u64, available: u64 },

    #[error(transparent)]
    CryptoError(#[from] CryptoError),

    #[error(transparent)]
    StateError(#[from] StateError),
}

// -----------------------------------------------------------------------------
// Core Logic
// -----------------------------------------------------------------------------

/// Computes the state hash (SHA-256 of canonical binary serialization)
pub fn compute_state_hash(state: &State) -> StateHash {
    let bytes = state.serialize_state_canonical();
    StateHash(sha256(&bytes))
}

/// Executes transactions and applies block reward to generate a new state.
/// This is used by block proposers to compute the state hash for a new block.
/// It performs the same state transitions as apply_block but without block-level validation (signatures, height, etc.).
pub fn execute_proposal(
    state: &State,
    transactions: &[Transaction],
    proposer_id: &ValidatorId,
) -> Result<(State, StateHash), ExecutionError> {
    let mut new_state = state.clone();

    // 7. For each transaction
    for tx in transactions {
        // a-e. Validate transaction
        validate_transaction(state, tx)?;
        // f-i. Apply transaction
        apply_transaction(&mut new_state, tx)?;
    }

    // 8. Apply block reward
    let proposer_account = new_state
        .get_validator(proposer_id)
        .ok_or(ExecutionError::UnknownValidator { id: *proposer_id })?
        .account_id;
    new_state.apply_reward(&proposer_account, new_state.block_reward)?;

    // 9. Verify economic invariants
    new_state
        .verify_invariants()
        .map_err(ExecutionError::StateError)?;

    // 10. Compute state hash
    let state_hash = compute_state_hash(&new_state);

    Ok((new_state, state_hash))
}

/// Applies a block to the previous state, returning the new state or an error.
/// This function implements the 10-step procedure from PROTOCOL.md Section 7.2.
pub fn apply_block(
    previous_state: &State,
    block: &Block,
    previous_block_hash: &BlockHash,
    previous_height: u64,
) -> Result<State, ExecutionError> {
    // 1. Validate block height == previous height + 1
    let expected_height = previous_height + 1;
    if block.height != expected_height {
        return Err(ExecutionError::InvalidHeight {
            expected: expected_height,
            got: block.height,
        });
    }

    // 2. Validate parent_hash matches hash of previous block
    if block.parent_hash != *previous_block_hash {
        return Err(ExecutionError::InvalidParentHash {
            expected: *previous_block_hash,
            got: block.parent_hash,
        });
    }

    // 3. Validate epoch == 0 (v1 only)
    if block.epoch != 0 {
        return Err(ExecutionError::InvalidEpoch {
            expected: 0,
            got: block.epoch,
        });
    }

    // 4. Validate proposer is an active validator
    // Primary proposer is select_proposer(height).
    // Fallback proposers are determined by (height + attempt) % validator_count.
    // While this function accepts any active validator (as 'attempt' is not in the block),
    // the consensus layer ensures only the correct deterministic proposer for the current
    // round receives a quorum of votes.
    let expected_proposer = select_proposer(previous_state, block.height)?;
    if block.proposer_id != expected_proposer {
        let validator = previous_state.get_validator(&block.proposer_id).ok_or(
            ExecutionError::UnknownValidator {
                id: block.proposer_id,
            },
        )?;
        if !validator.active {
            return Err(ExecutionError::InactiveValidator {
                id: block.proposer_id,
            });
        }
    }

    // 5. Validate block limits
    validate_block_limits(block)?;

    // 6. Validate quorum
    verify_quorum(previous_state, block)?;

    // Clone state for atomic application
    let mut new_state = previous_state.clone();

    // 7. Process transactions
    for tx in &block.transactions {
        // a-e. Validate transaction
        validate_transaction(&new_state, tx)?;

        // f-i. Apply transaction (atomic steps)
        apply_transaction(&mut new_state, tx)?;
    }

    // 8. Apply block reward
    // Get proposer's account ID (validator must exist as checked in step 4)
    let proposer_account = new_state
        .get_validator(&block.proposer_id)
        .ok_or(ExecutionError::UnknownValidator {
            id: block.proposer_id,
        })?
        .account_id;
    new_state.apply_reward(&proposer_account, new_state.block_reward)?;

    // 9. Verify economic invariants
    new_state.verify_invariants()?;

    // 10. Compute and verify state hash
    let computed_hash = compute_state_hash(&new_state);
    if computed_hash != block.state_hash {
        return Err(ExecutionError::StateHashMismatch {
            expected: block.state_hash,
            computed: computed_hash,
        });
    }

    Ok(new_state)
}

// -----------------------------------------------------------------------------
// v2 Execution (Staking)
// -----------------------------------------------------------------------------

/// Applies a block with protocol version awareness.
/// For v1 blocks (height < activation): delegates to apply_block (v1 rules).
/// For v2 blocks (height >= activation): applies staking logic, unbonding releases,
/// and processes Stake/Unstake transactions in addition to Transfer.
pub fn apply_block_v2(
    previous_state: &State,
    staking_state: &StakingState,
    block: &Block,
    previous_block_hash: &BlockHash,
    previous_height: u64,
) -> Result<(State, StakingState), ExecutionError> {
    let version = ProtocolVersion::for_height(block.height);

    match version {
        ProtocolVersion::V1 => {
            for tx in &block.transactions {
                if tx.tx_type != TransactionType::Transfer {
                    return Err(ExecutionError::V2TransactionInV1Block {
                        tx_type: tx.tx_type,
                        height: block.height,
                    });
                }
            }
            let new_state =
                apply_block(previous_state, block, previous_block_hash, previous_height)?;
            Ok((new_state, staking_state.clone()))
        }
        ProtocolVersion::V2 => {
            apply_block_v2_inner(
                previous_state,
                staking_state,
                block,
                previous_block_hash,
                previous_height,
            )
        }
    }
}

/// Inner implementation for v2 block application.
fn apply_block_v2_inner(
    previous_state: &State,
    staking_state: &StakingState,
    block: &Block,
    previous_block_hash: &BlockHash,
    previous_height: u64,
) -> Result<(State, StakingState), ExecutionError> {
    let expected_height = previous_height + 1;
    if block.height != expected_height {
        return Err(ExecutionError::InvalidHeight {
            expected: expected_height,
            got: block.height,
        });
    }

    if block.parent_hash != *previous_block_hash {
        return Err(ExecutionError::InvalidParentHash {
            expected: *previous_block_hash,
            got: block.parent_hash,
        });
    }

    if block.epoch != 0 {
        return Err(ExecutionError::InvalidEpoch {
            expected: 0,
            got: block.epoch,
        });
    }

    let expected_proposer = select_proposer(previous_state, block.height)?;
    if block.proposer_id != expected_proposer {
        let validator = previous_state.get_validator(&block.proposer_id).ok_or(
            ExecutionError::UnknownValidator {
                id: block.proposer_id,
            },
        )?;
        if !validator.active {
            return Err(ExecutionError::InactiveValidator {
                id: block.proposer_id,
            });
        }
    }

    validate_block_limits(block)?;
    verify_quorum(previous_state, block)?;

    let mut new_state = previous_state.clone();
    let mut new_staking = staking_state.clone();

    let released = new_staking.release_unbonded(block.height);
    for (vid, amount) in released {
        let account_id = find_validator_account(&new_state, &vid)?;
        new_state.apply_reward(&account_id, amount)?;
    }

    for tx in &block.transactions {
        match tx.tx_type {
            TransactionType::Transfer => {
                validate_transaction(&new_state, tx)?;
                apply_transaction(&mut new_state, tx)?;
            }
            TransactionType::Stake => {
                validate_stake_transaction(&new_state, &new_staking, tx)?;
                apply_stake_transaction(&mut new_state, &mut new_staking, tx)?;
            }
            TransactionType::Unstake => {
                validate_unstake_transaction(&new_state, &new_staking, tx)?;
                apply_unstake_transaction(&mut new_state, &mut new_staking, tx, block.height)?;
            }
            TransactionType::SlashEvidence => {
                return Err(ExecutionError::SlashEvidenceNotImplemented);
            }
        }
    }

    let proposer_account = new_state
        .get_validator(&block.proposer_id)
        .ok_or(ExecutionError::UnknownValidator {
            id: block.proposer_id,
        })?
        .account_id;
    new_state.apply_reward(&proposer_account, new_state.block_reward)?;

    verify_staking_invariants(&new_state, &new_staking)?;

    let computed_hash = compute_state_hash(&new_state);
    if computed_hash != block.state_hash {
        return Err(ExecutionError::StateHashMismatch {
            expected: block.state_hash,
            computed: computed_hash,
        });
    }

    Ok((new_state, new_staking))
}

/// Executes a v2 proposal (used by proposers to compute state hash).
pub fn execute_proposal_v2(
    state: &State,
    staking_state: &StakingState,
    transactions: &[Transaction],
    proposer_id: &ValidatorId,
    height: u64,
) -> Result<(State, StakingState, StateHash), ExecutionError> {
    let version = ProtocolVersion::for_height(height);

    match version {
        ProtocolVersion::V1 => {
            for tx in transactions {
                if tx.tx_type != TransactionType::Transfer {
                    return Err(ExecutionError::V2TransactionInV1Block {
                        tx_type: tx.tx_type,
                        height,
                    });
                }
            }
            let (new_state, hash) = execute_proposal(state, transactions, proposer_id)?;
            Ok((new_state, staking_state.clone(), hash))
        }
        ProtocolVersion::V2 => {
            let mut new_state = state.clone();
            let mut new_staking = staking_state.clone();

            let released = new_staking.release_unbonded(height);
            for (vid, amount) in released {
                let account_id = find_validator_account(&new_state, &vid)?;
                new_state.apply_reward(&account_id, amount)?;
            }

            for tx in transactions {
                match tx.tx_type {
                    TransactionType::Transfer => {
                        validate_transaction(state, tx)?;
                        apply_transaction(&mut new_state, tx)?;
                    }
                    TransactionType::Stake => {
                        validate_stake_transaction(&new_state, &new_staking, tx)?;
                        apply_stake_transaction(&mut new_state, &mut new_staking, tx)?;
                    }
                    TransactionType::Unstake => {
                        validate_unstake_transaction(&new_state, &new_staking, tx)?;
                        apply_unstake_transaction(
                            &mut new_state,
                            &mut new_staking,
                            tx,
                            height,
                        )?;
                    }
                    TransactionType::SlashEvidence => {
                        return Err(ExecutionError::SlashEvidenceNotImplemented);
                    }
                }
            }

            let proposer_account = new_state
                .get_validator(proposer_id)
                .ok_or(ExecutionError::UnknownValidator { id: *proposer_id })?
                .account_id;
            new_state.apply_reward(&proposer_account, new_state.block_reward)?;

            verify_staking_invariants(&new_state, &new_staking)?;

            let state_hash = compute_state_hash(&new_state);
            Ok((new_state, new_staking, state_hash))
        }
    }
}

/// Finds the account_id associated with a validator_id.
fn find_validator_account(
    state: &State,
    validator_id: &ValidatorId,
) -> Result<AccountId, ExecutionError> {
    let validator = state
        .get_validator(validator_id)
        .ok_or(ExecutionError::UnknownValidator {
            id: *validator_id,
        })?;
    Ok(validator.account_id)
}

/// Validates a stake transaction.
fn validate_stake_transaction(
    state: &State,
    staking: &StakingState,
    tx: &Transaction,
) -> Result<(), ExecutionError> {
    verify_transaction_signature(tx).map_err(ExecutionError::CryptoError)?;

    let sender_acc = state
        .get_account(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?;

    if tx.nonce != sender_acc.nonce {
        return Err(ExecutionError::InvalidNonce {
            expected: sender_acc.nonce,
            got: tx.nonce,
        });
    }

    if tx.amount == 0 {
        return Err(ExecutionError::ZeroAmount);
    }

    if tx.amount < MIN_VALIDATOR_STAKE {
        return Err(ExecutionError::StakeBelowMinimum {
            amount: tx.amount,
            minimum: MIN_VALIDATOR_STAKE,
        });
    }

    if sender_acc.balance < tx.amount {
        return Err(ExecutionError::InsufficientBalance {
            account: tx.sender,
            required: tx.amount,
            available: sender_acc.balance,
        });
    }

    let vid = ValidatorId(tx.sender.0);
    let is_validator_account = state
        .validators
        .values()
        .any(|v| v.account_id == tx.sender);
    if !is_validator_account {
        return Err(ExecutionError::NotValidatorAccount { sender: tx.sender });
    }

    if staking.stakes.contains_key(&vid) {
        return Err(ExecutionError::AlreadyStaked {
            account: tx.sender,
        });
    }

    Ok(())
}

/// Applies a stake transaction to state and staking state.
fn apply_stake_transaction(
    state: &mut State,
    staking: &mut StakingState,
    tx: &Transaction,
) -> Result<(), ExecutionError> {
    let sender = state
        .get_account_mut(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?;

    sender.balance = sender
        .balance
        .checked_sub(tx.amount)
        .ok_or(ExecutionError::Underflow)?;

    sender.nonce = sender
        .nonce
        .checked_add(1)
        .ok_or(ExecutionError::Overflow)?;

    let vid = ValidatorId(tx.sender.0);
    staking
        .apply_stake(vid, StakeAmount(tx.amount))
        .map_err(ExecutionError::StateError)?;

    Ok(())
}

/// Validates an unstake transaction.
fn validate_unstake_transaction(
    state: &State,
    staking: &StakingState,
    tx: &Transaction,
) -> Result<(), ExecutionError> {
    verify_transaction_signature(tx).map_err(ExecutionError::CryptoError)?;

    let sender_acc = state
        .get_account(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?;

    if tx.nonce != sender_acc.nonce {
        return Err(ExecutionError::InvalidNonce {
            expected: sender_acc.nonce,
            got: tx.nonce,
        });
    }

    if tx.amount == 0 {
        return Err(ExecutionError::ZeroAmount);
    }

    let vid = ValidatorId(tx.sender.0);
    let staked = staking
        .stakes
        .get(&vid)
        .ok_or(ExecutionError::NoActiveStake {
            account: tx.sender,
        })?;

    if tx.amount > staked.0 {
        return Err(ExecutionError::InsufficientStake {
            requested: tx.amount,
            available: staked.0,
        });
    }

    Ok(())
}

/// Applies an unstake transaction to state and staking state.
fn apply_unstake_transaction(
    state: &mut State,
    staking: &mut StakingState,
    tx: &Transaction,
    current_height: u64,
) -> Result<(), ExecutionError> {
    let sender = state
        .get_account_mut(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?;

    sender.nonce = sender
        .nonce
        .checked_add(1)
        .ok_or(ExecutionError::Overflow)?;

    let vid = ValidatorId(tx.sender.0);
    staking
        .apply_unstake(vid, tx.amount, current_height)
        .map_err(ExecutionError::StateError)?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

pub fn select_proposer(state: &State, height: u64) -> Result<ValidatorId, ExecutionError> {
    let active = state.active_validators();
    if active.is_empty() {
        return Err(ExecutionError::NoActiveValidators);
    }

    // active is already sorted by ValidatorId (BTreeMap iteration order)
    let index = (height as usize) % active.len();
    Ok(*active[index].0)
}

pub fn select_fallback_proposer(
    state: &State,
    height: u64,
    attempt: u64,
) -> Result<ValidatorId, ExecutionError> {
    let active = state.active_validators();
    if active.is_empty() {
        return Err(ExecutionError::NoActiveValidators);
    }

    // active is already sorted by ValidatorId (BTreeMap iteration order)
    let index = ((height + attempt) as usize) % active.len();
    Ok(*active[index].0)
}

fn validate_block_limits(block: &Block) -> Result<(), ExecutionError> {
    if block.transactions.len() > MAX_TRANSACTIONS_PER_BLOCK {
        return Err(ExecutionError::BlockTooManyTransactions {
            count: block.transactions.len(),
            max: MAX_TRANSACTIONS_PER_BLOCK,
        });
    }

    let bytes = serialize_block_canonical(block);
    // serialize_block_canonical excludes signatures, so we must add them manually
    // Each signature is 96 bytes (32 ValidatorId + 64 Signature)
    let signatures_size = block.signatures.len() * 96;
    let total_size = bytes.len() + signatures_size;

    if total_size > MAX_BLOCK_SIZE_BYTES {
        return Err(ExecutionError::BlockTooLarge {
            size: total_size,
            max: MAX_BLOCK_SIZE_BYTES,
        });
    }

    Ok(())
}

fn verify_quorum(state: &State, block: &Block) -> Result<(), ExecutionError> {
    let total_power = state
        .total_voting_power()
        .map_err(|_| ExecutionError::Overflow)?;
    let mut collected_power: u64 = 0;
    let mut seen_validators = HashSet::new();

    // Vote message = SHA-256(block_hash || height)
    let block_hash = compute_block_hash(block);

    for sig in &block.signatures {
        if !seen_validators.insert(&sig.validator_id) {
            return Err(ExecutionError::DuplicateSignature {
                validator: sig.validator_id,
            });
        }

        let validator =
            state
                .get_validator(&sig.validator_id)
                .ok_or(ExecutionError::UnknownValidator {
                    id: sig.validator_id,
                })?;

        if !validator.active {
            return Err(ExecutionError::InactiveValidator {
                id: sig.validator_id,
            });
        }

        // Verify signature
        // ValidatorId is the Ed25519 public key
        let public_key = axiom_primitives::PublicKey(sig.validator_id.0);
        verify_vote(&public_key, &block_hash, block.height, &sig.signature)?;

        collected_power = collected_power
            .checked_add(validator.voting_power)
            .ok_or(ExecutionError::Overflow)?;
    }

    // Quorum: collected > 2/3 total
    // <=> 3 * collected > 2 * total
    let lhs = collected_power
        .checked_mul(3)
        .ok_or(ExecutionError::Overflow)?;
    let rhs = total_power.checked_mul(2).ok_or(ExecutionError::Overflow)?;

    if lhs <= rhs {
        return Err(ExecutionError::QuorumNotMet {
            collected: collected_power,
            required: (rhs / 3) + 1,
        });
    }

    Ok(())
}

fn validate_transaction(state: &State, tx: &Transaction) -> Result<(), ExecutionError> {
    // a. Validate signature
    verify_transaction_signature(tx).map_err(ExecutionError::CryptoError)?;

    // b. Validate sender exists
    let sender_acc = state
        .get_account(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?;

    // c. Validate nonce matches
    if tx.nonce != sender_acc.nonce {
        return Err(ExecutionError::InvalidNonce {
            expected: sender_acc.nonce,
            got: tx.nonce,
        });
    }

    // d. Validate amount > 0
    if tx.amount == 0 {
        return Err(ExecutionError::ZeroAmount);
    }

    // e. Validate sender balance >= amount
    if sender_acc.balance < tx.amount {
        return Err(ExecutionError::InsufficientBalance {
            account: tx.sender,
            required: tx.amount,
            available: sender_acc.balance,
        });
    }

    Ok(())
}

fn apply_transaction(state: &mut State, tx: &Transaction) -> Result<(), ExecutionError> {
    // f. If recipient doesn't exist, auto-create
    if state.get_account(&tx.recipient).is_none() {
        let new_acc = Account {
            balance: 0,
            nonce: 0,
        };
        state.create_account(tx.recipient, new_acc);
    }

    // g. Decrease sender balance
    let sender = state
        .get_account_mut(&tx.sender)
        .ok_or(ExecutionError::SenderNotFound { sender: tx.sender })?; // Should exist from validation

    sender.balance = sender
        .balance
        .checked_sub(tx.amount)
        .ok_or(ExecutionError::Underflow)?;

    // i. Increment sender nonce
    sender.nonce = sender
        .nonce
        .checked_add(1)
        .ok_or(ExecutionError::Overflow)?;

    // h. Increase recipient balance
    let recipient = state
        .get_account_mut(&tx.recipient)
        .ok_or(ExecutionError::Overflow)?; // Should exist now

    recipient.balance = recipient
        .balance
        .checked_add(tx.amount)
        .ok_or(ExecutionError::Overflow)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_crypto::test_keypair;
    use axiom_primitives::{GenesisAccount, GenesisConfig, GenesisValidator, Signature, TransactionType, V2_ACTIVATION_HEIGHT, UNBONDING_PERIOD};

    fn create_genesis_state() -> (State, axiom_crypto::PrivateKey, ValidatorId, AccountId) {
        let (sk, pk) = test_keypair("val1");
        let val_id = ValidatorId(pk.0);
        let acc_id = AccountId(pk.0); // Validator account same as ID for simplicity

        let genesis = GenesisConfig {
            total_supply: 1000,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 1000,
                nonce: 0,
            }],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 100,
                account_id: acc_id,
                active: true,
            }],
        };

        (State::from_genesis(&genesis).unwrap(), sk, val_id, acc_id)
    }

    fn sign_block(sk: &axiom_crypto::PrivateKey, block: &mut Block, val_id: &ValidatorId) {
        let block_hash = compute_block_hash(block);
        let sig = axiom_crypto::sign_vote(sk, &block_hash, block.height);
        block.signatures.push(axiom_primitives::ValidatorSignature {
            validator_id: *val_id,
            signature: sig,
        });
    }

    #[test]
    fn test_apply_valid_empty_block() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]), // Placeholder
            timestamp: 0,
        };

        // Pre-calculate expected state hash
        let mut expected_state = state.clone();
        expected_state.apply_reward(&acc_id, 10).unwrap();
        block.state_hash = compute_state_hash(&expected_state);

        sign_block(&sk, &mut block, &val_id);

        let new_state = apply_block(&state, &block, &prev_hash, 0).unwrap();
        assert_eq!(new_state.total_supply, 1010);
        assert_eq!(new_state.get_account(&acc_id).unwrap().balance, 1010);
    }

    #[test]
    fn test_apply_valid_transfer() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);
        let (_recipient_sk, recipient_pk) = test_keypair("recipient");
        let recipient_id = AccountId(recipient_pk.0);

        let tx = Transaction {
            sender: acc_id,
            recipient: recipient_id,
            amount: 50,
            nonce: 0,
            signature: Signature([0u8; 64]), // Placeholder
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        // Expected state:
        // Sender: 1000 - 50 + 10 (reward) = 960
        // Recipient: 50
        // Total Supply: 1010
        let mut expected_state = state.clone();
        // Manually apply for hash
        let sender = expected_state.get_account_mut(&acc_id).unwrap();
        sender.balance -= 50;
        sender.nonce += 1;

        expected_state.create_account(
            recipient_id,
            Account {
                balance: 50,
                nonce: 0,
            },
        );

        expected_state.apply_reward(&acc_id, 10).unwrap();

        block.state_hash = compute_state_hash(&expected_state);
        sign_block(&sk, &mut block, &val_id);

        let new_state = apply_block(&state, &block, &prev_hash, 0).unwrap();

        assert_eq!(new_state.get_account(&acc_id).unwrap().balance, 960);
        assert_eq!(new_state.get_account(&recipient_id).unwrap().balance, 50);
        assert_eq!(new_state.total_supply, 1010);
    }

    #[test]
    fn test_reject_invalid_signature() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        // Transaction signed by wrong key
        let (wrong_sk, _) = test_keypair("wrong");
        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id, // Self transfer
            amount: 10,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&wrong_sk, &tx); // Wrong key!

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        // We don't care about state hash here, it should fail before
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::CryptoError(_))));
    }

    #[test]
    fn test_reject_insufficient_balance() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 2000, // Balance is 1000
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(
            res,
            Err(ExecutionError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn test_reject_invalid_nonce() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 10,
            nonce: 1, // Invalid, expected 0
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::InvalidNonce { .. })));
    }

    #[test]
    fn test_reject_zero_amount() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 0,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::ZeroAmount)));
    }

    #[test]
    fn test_reject_wrong_height() {
        let (state, sk, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 2, // Expected 1
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        // Fix state hash so it doesn't fail on that (though it shouldn't reach it)
        let mut expected_state = state.clone();
        expected_state
            .apply_reward(&state.get_validator(&val_id).unwrap().account_id, 10)
            .unwrap();
        block.state_hash = compute_state_hash(&expected_state);

        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::InvalidHeight { .. })));
    }

    #[test]
    fn test_reject_wrong_state_hash() {
        let (state, sk, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0xff; 32]), // Wrong hash
            timestamp: 0,
        };

        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::StateHashMismatch { .. })));
    }

    #[test]
    fn test_block_limit_too_many_txs() {
        let (state, _, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        // Create 1001 dummy transactions
        // We don't need valid signatures because limit check (step 5) happens before tx validation (step 7)
        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 1,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let transactions = vec![tx; MAX_TRANSACTIONS_PER_BLOCK + 1];

        let block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions,
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(
            res,
            Err(ExecutionError::BlockTooManyTransactions { .. })
        ));
    }

    #[test]
    fn test_account_auto_creation_explicit() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let (_recipient_sk, recipient_pk) = test_keypair("new_user");
        let recipient_id = AccountId(recipient_pk.0);

        // Verify account does not exist yet
        assert!(state.get_account(&recipient_id).is_none());

        let tx = Transaction {
            sender: acc_id,
            recipient: recipient_id,
            amount: 100,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        // Compute expected hash
        let mut expected_state = state.clone();
        expected_state.create_account(
            recipient_id,
            Account {
                balance: 100,
                nonce: 0,
            },
        );
        let sender = expected_state.get_account_mut(&acc_id).unwrap();
        sender.balance -= 100;
        sender.nonce += 1;
        expected_state.apply_reward(&acc_id, 10).unwrap();

        block.state_hash = compute_state_hash(&expected_state);
        sign_block(&sk, &mut block, &val_id);

        let new_state = apply_block(&state, &block, &prev_hash, 0).unwrap();

        // Verify account exists now
        assert!(new_state.get_account(&recipient_id).is_some());
        assert_eq!(new_state.get_account(&recipient_id).unwrap().balance, 100);
    }

    #[test]
    fn test_reject_wrong_parent_hash() {
        let (state, sk, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);
        let wrong_hash = BlockHash([1u8; 32]);

        let mut block = Block {
            parent_hash: wrong_hash, // Expected 00...
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::InvalidParentHash { .. })));
    }

    #[test]
    fn test_quorum_enforcement() {
        let (state, _, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        let block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![], // No signatures!
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        // Don't sign block

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::QuorumNotMet { .. })));
    }

    #[test]
    fn test_atomicity_failure_reverts_state() {
        // Block with 2 txs: first valid, second invalid (insufficient balance)
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        // Tx 1: Valid
        let tx1 = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 10,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx1 = tx1.clone();
        signed_tx1.signature = axiom_crypto::sign_transaction(&sk, &tx1);

        // Tx 2: Invalid (Amount > Balance)
        let tx2 = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 2000,
            nonce: 1,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx2 = tx2.clone();
        signed_tx2.signature = axiom_crypto::sign_transaction(&sk, &tx2);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx1, signed_tx2],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(
            res,
            Err(ExecutionError::InsufficientBalance { .. })
        ));

        // Verify state is unchanged (nonce of sender is still 0, balance still 1000)
        assert_eq!(state.get_account(&acc_id).unwrap().nonce, 0);
        assert_eq!(state.get_account(&acc_id).unwrap().balance, 1000);
    }

    #[test]
    fn test_block_limit_too_large_bytes() {
        let (state, _, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        // Create a block with 0 txs but huge number of signatures to exceed 1MB
        // Each signature is ~96 bytes. 20,000 signatures ~ 1.9MB
        let signatures = vec![
            axiom_primitives::ValidatorSignature {
                validator_id: val_id,
                signature: Signature([0u8; 64]),
            };
            20_000
        ];

        let block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures,
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        let res = apply_block(&state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::BlockTooLarge { .. })));
    }

    #[test]
    fn test_verify_invariants_failure() {
        let (state, sk, val_id, _) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);

        // Corrupt the state manually (Total Supply != Sum of Balances)
        let mut corrupted_state = state.clone();
        corrupted_state.total_supply += 1; // Artificially increase supply without adding balance

        // Valid empty block
        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        // Compute state hash based on corrupted state + reward
        let mut expected_state = corrupted_state.clone();
        expected_state
            .apply_reward(&state.get_validator(&val_id).unwrap().account_id, 10)
            .unwrap();
        block.state_hash = compute_state_hash(&expected_state);

        sign_block(&sk, &mut block, &val_id);

        // Should fail at verify_invariants step
        let res = apply_block(&corrupted_state, &block, &prev_hash, 0);
        assert!(matches!(res, Err(ExecutionError::StateError(_))));
    }

    fn create_rich_genesis_state() -> (State, axiom_crypto::PrivateKey, ValidatorId, AccountId) {
        let (sk, pk) = test_keypair("val1");
        let val_id = ValidatorId(pk.0);
        let acc_id = AccountId(pk.0);

        let genesis = GenesisConfig {
            total_supply: 1_000_000,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 1_000_000,
                nonce: 0,
            }],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 100,
                account_id: acc_id,
                active: true,
            }],
        };

        (State::from_genesis(&genesis).unwrap(), sk, val_id, acc_id)
    }

    #[test]
    fn test_v2_reject_stake_in_v1_block() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block_v2(&state, &staking, &block, &prev_hash, 0);
        assert!(matches!(
            res,
            Err(ExecutionError::V2TransactionInV1Block { .. })
        ));
    }

    #[test]
    fn test_v2_reject_unstake_in_v1_block() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Unstake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![signed_tx],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk, &mut block, &val_id);

        let res = apply_block_v2(&state, &staking, &block, &prev_hash, 0);
        assert!(matches!(
            res,
            Err(ExecutionError::V2TransactionInV1Block { .. })
        ));
    }

    #[test]
    fn test_v2_reject_slash_evidence() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::SlashEvidence,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let res = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        );
        assert!(matches!(
            res,
            Err(ExecutionError::SlashEvidenceNotImplemented)
        ));
    }

    #[test]
    fn test_v2_stake_success() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let (new_state, new_staking, _hash) = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        )
        .unwrap();

        assert_eq!(
            new_state.get_account(&acc_id).unwrap().balance,
            1_000_000 - 100_000 + 10
        );
        let vid = ValidatorId(acc_id.0);
        assert_eq!(new_staking.stakes.get(&vid).unwrap().0, 100_000);
    }

    #[test]
    fn test_v2_stake_insufficient_balance() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let res = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        );
        assert!(matches!(
            res,
            Err(ExecutionError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn test_v2_stake_below_minimum() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 50_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let res = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        );
        assert!(matches!(
            res,
            Err(ExecutionError::StakeBelowMinimum { .. })
        ));
    }

    #[test]
    fn test_v2_unstake_success() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let stake_tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_stake = stake_tx.clone();
        signed_stake.signature = axiom_crypto::sign_transaction(&sk, &stake_tx);

        let (state2, staking2, _) = execute_proposal_v2(
            &state,
            &staking,
            &[signed_stake],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        )
        .unwrap();

        let unstake_tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 1,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Unstake,
        };
        let mut signed_unstake = unstake_tx.clone();
        signed_unstake.signature = axiom_crypto::sign_transaction(&sk, &unstake_tx);

        let (_state3, staking3, _) = execute_proposal_v2(
            &state2,
            &staking2,
            &[signed_unstake],
            &val_id,
            V2_ACTIVATION_HEIGHT + 1,
        )
        .unwrap();

        let vid = ValidatorId(acc_id.0);
        assert!(!staking3.stakes.contains_key(&vid));
        assert_eq!(staking3.unbonding_queue.len(), 1);
        assert_eq!(staking3.unbonding_queue[0].amount.0, 100_000);
    }

    #[test]
    fn test_v2_unstake_no_stake() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Unstake,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let res = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        );
        assert!(matches!(res, Err(ExecutionError::NoActiveStake { .. })));
    }

    #[test]
    fn test_v2_unbonding_release() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let stake_tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Stake,
        };
        let mut signed_stake = stake_tx.clone();
        signed_stake.signature = axiom_crypto::sign_transaction(&sk, &stake_tx);

        let (state2, staking2, _) = execute_proposal_v2(
            &state,
            &staking,
            &[signed_stake],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        )
        .unwrap();

        let unstake_tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 100_000,
            nonce: 1,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Unstake,
        };
        let mut signed_unstake = unstake_tx.clone();
        signed_unstake.signature = axiom_crypto::sign_transaction(&sk, &unstake_tx);

        let unstake_height = V2_ACTIVATION_HEIGHT + 1;
        let (_state3, staking3, _) = execute_proposal_v2(
            &state2,
            &staking2,
            &[signed_unstake],
            &val_id,
            unstake_height,
        )
        .unwrap();

        assert_eq!(staking3.unbonding_queue.len(), 1);
        assert_eq!(staking3.unbonding_queue[0].amount.0, 100_000);
        assert_eq!(
            staking3.unbonding_queue[0].release_height,
            unstake_height + UNBONDING_PERIOD
        );

        let release_height = unstake_height + UNBONDING_PERIOD;
        let mut release_staking = staking3.clone();
        let released = release_staking.release_unbonded(release_height);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].0, ValidatorId(acc_id.0));
        assert_eq!(released[0].1, 100_000);
        assert!(release_staking.unbonding_queue.is_empty());
    }

    #[test]
    fn test_v1_block_unchanged_with_v2_available() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let prev_hash = BlockHash([0u8; 32]);
        let staking = StakingState::empty();

        let mut block = Block {
            parent_hash: prev_hash,
            height: 1,
            epoch: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };

        let mut expected_state = state.clone();
        expected_state.apply_reward(&acc_id, 10).unwrap();
        block.state_hash = compute_state_hash(&expected_state);
        sign_block(&sk, &mut block, &val_id);

        let (new_state, new_staking) =
            apply_block_v2(&state, &staking, &block, &prev_hash, 0).unwrap();
        assert_eq!(new_state.total_supply, 1010);
        assert_eq!(new_state.get_account(&acc_id).unwrap().balance, 1010);
        assert!(new_staking.is_empty());
    }

    #[test]
    fn test_v2_transfer_still_works() {
        let (state, sk, val_id, acc_id) = create_rich_genesis_state();
        let staking = StakingState::new_active();

        let (_recipient_sk, recipient_pk) = test_keypair("recipient");
        let recipient_id = AccountId(recipient_pk.0);

        let tx = Transaction {
            sender: acc_id,
            recipient: recipient_id,
            amount: 500,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let (new_state, _new_staking, _hash) = execute_proposal_v2(
            &state,
            &staking,
            &[signed_tx],
            &val_id,
            V2_ACTIVATION_HEIGHT,
        )
        .unwrap();

        assert_eq!(
            new_state.get_account(&acc_id).unwrap().balance,
            1_000_000 - 500 + 10
        );
        assert_eq!(
            new_state.get_account(&recipient_id).unwrap().balance,
            500
        );
    }

    #[test]
    fn test_protocol_v1_locked_vectors_genesis_and_vectors() {
        fn seed32(hex_str: &str) -> [u8; 32] {
            axiom_primitives::from_hex(hex_str)
                .expect("Invalid hex")
                .try_into()
                .expect("Expected 32 bytes")
        }

        let genesis_json = include_str!("../../docs/reference_genesis.json");
        let genesis =
            axiom_primitives::deserialize_genesis_json(genesis_json).expect("Invalid genesis JSON");

        let state = State::from_genesis(&genesis).expect("Failed to construct genesis state");
        let genesis_hash = compute_state_hash(&state);
        assert_eq!(
            genesis_hash.to_string(),
            "c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761"
        );

        let sk_val_1 = axiom_crypto::PrivateKey::from_bytes(&seed32(
            "eed1444f431a29ddaba560d09559f7b3453cc1def5861ab51bcd3344dae18834",
        ));
        let sk_val_2 = axiom_crypto::PrivateKey::from_bytes(&seed32(
            "9bd3bf36c5da99993f250e5b2e558e6768583ed5bbbd24a39560fca381b3c369",
        ));
        let sk_val_3 = axiom_crypto::PrivateKey::from_bytes(&seed32(
            "2a8e0ea62396cbe5821e10a3700ee4da1a96eea2bed02c6f28d16591e682e3cb",
        ));
        let sk_val_4 = axiom_crypto::PrivateKey::from_bytes(&seed32(
            "139a29f05f0426440423e577fe65810d96d8dd4418f4f4d2226b04f2b5a40712",
        ));

        let val_1 = ValidatorId(sk_val_1.verifying_key().to_bytes());
        let val_2 = ValidatorId(sk_val_2.verifying_key().to_bytes());
        let val_3 = ValidatorId(sk_val_3.verifying_key().to_bytes());
        let val_4 = ValidatorId(sk_val_4.verifying_key().to_bytes());

        assert_eq!(
            val_1.to_string(),
            "e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5"
        );
        assert_eq!(
            val_2.to_string(),
            "97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3"
        );
        assert_eq!(
            val_3.to_string(),
            "b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf"
        );
        assert_eq!(
            val_4.to_string(),
            "9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0"
        );

        let account_a = AccountId(val_2.0);
        let account_b = AccountId(val_4.0);
        let account_c = AccountId(val_3.0);
        let account_d = AccountId(val_1.0);

        let expected_proposer_h1 =
            select_proposer(&state, 1).expect("Failed to select proposer at height 1");
        assert_eq!(expected_proposer_h1, val_4);

        let (state_after_b1, state_hash_b1) =
            execute_proposal(&state, &[], &val_4).expect("Failed to execute empty proposal");
        assert_eq!(
            state_hash_b1.to_string(),
            "3803627a326ce03883ca996f9b8bcfd41ff3f5cf51ae21a6726207e9221b9514"
        );

        let prev_hash_0 = BlockHash([0u8; 32]);

        let mut block_1 = Block {
            parent_hash: prev_hash_0,
            height: 1,
            epoch: 0,
            proposer_id: val_4,
            transactions: vec![],
            signatures: vec![],
            state_hash: state_hash_b1,
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_1, &val_1);
        sign_block(&sk_val_2, &mut block_1, &val_2);
        sign_block(&sk_val_3, &mut block_1, &val_3);

        let block_hash_1 = compute_block_hash(&block_1);
        assert_eq!(
            block_hash_1.to_string(),
            "1fa4adaedc4ff6776c22aba6185966736d031a4e981791d7b711833e06838cfe"
        );

        let applied_state_b1 = apply_block(&state, &block_1, &prev_hash_0, 0)
            .expect("Block Vector 1 should be accepted");
        assert_eq!(compute_state_hash(&applied_state_b1), state_hash_b1);
        assert_eq!(
            applied_state_b1.get_account(&account_b).unwrap().balance,
            1_000_010
        );
        assert_eq!(applied_state_b1.total_supply, 4_000_010);

        let mut block_2_invalid_quorum = Block {
            parent_hash: block_hash_1,
            height: 2,
            epoch: 0,
            proposer_id: val_3,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk_val_3, &mut block_2_invalid_quorum, &val_3);
        sign_block(&sk_val_1, &mut block_2_invalid_quorum, &val_1);
        let res = apply_block(&state_after_b1, &block_2_invalid_quorum, &block_hash_1, 1);
        assert!(matches!(res, Err(ExecutionError::QuorumNotMet { .. })));
        assert_eq!(compute_state_hash(&state_after_b1), state_hash_b1);

        let mut block_1_duplicate_height = Block {
            parent_hash: prev_hash_0,
            height: 1,
            epoch: 0,
            proposer_id: val_4,
            transactions: vec![],
            signatures: vec![],
            state_hash: state_hash_b1,
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_1_duplicate_height, &val_1);
        sign_block(&sk_val_2, &mut block_1_duplicate_height, &val_2);
        sign_block(&sk_val_3, &mut block_1_duplicate_height, &val_3);
        let res = apply_block(
            &state_after_b1,
            &block_1_duplicate_height,
            &block_hash_1,
            1,
        );
        assert!(matches!(res, Err(ExecutionError::InvalidHeight { .. })));

        let (state_after_b2_fallback, state_hash_b2_fallback) =
            execute_proposal(&state_after_b1, &[], &val_2)
                .expect("Failed to execute empty fallback proposal");
        let mut block_2_fallback = Block {
            parent_hash: block_hash_1,
            height: 2,
            epoch: 0,
            proposer_id: val_2,
            transactions: vec![],
            signatures: vec![],
            state_hash: state_hash_b2_fallback,
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_2_fallback, &val_1);
        sign_block(&sk_val_2, &mut block_2_fallback, &val_2);
        sign_block(&sk_val_3, &mut block_2_fallback, &val_3);
        let applied_state_b2_fallback =
            apply_block(&state_after_b1, &block_2_fallback, &block_hash_1, 1)
                .expect("Block Vector 4 (fallback proposer) should be accepted");
        assert_eq!(
            compute_state_hash(&applied_state_b2_fallback),
            state_hash_b2_fallback
        );
        assert_eq!(
            compute_state_hash(&applied_state_b2_fallback),
            compute_state_hash(&state_after_b2_fallback)
        );
        assert_eq!(
            applied_state_b2_fallback
                .get_account(&account_a)
                .unwrap()
                .balance,
            1_000_010
        );
        assert_eq!(
            applied_state_b2_fallback
                .get_account(&account_b)
                .unwrap()
                .balance,
            1_000_010
        );
        assert_eq!(applied_state_b2_fallback.total_supply, 4_000_020);

        let mut block_2_wrong_epoch = Block {
            parent_hash: block_hash_1,
            height: 2,
            epoch: 1,
            proposer_id: val_3,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_2_wrong_epoch, &val_1);
        sign_block(&sk_val_2, &mut block_2_wrong_epoch, &val_2);
        sign_block(&sk_val_3, &mut block_2_wrong_epoch, &val_3);
        let res = apply_block(&state_after_b1, &block_2_wrong_epoch, &block_hash_1, 1);
        assert!(matches!(res, Err(ExecutionError::InvalidEpoch { .. })));

        let tx_1 = Transaction {
            sender: account_d,
            recipient: account_a,
            amount: 100_000,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx_1 = tx_1.clone();
        signed_tx_1.signature = axiom_crypto::sign_transaction(&sk_val_1, &tx_1);

        let (state_after_b2, state_hash_b2) = execute_proposal(
            &state_after_b1,
            std::slice::from_ref(&signed_tx_1),
            &val_3,
        )
        .expect("Failed to execute Transaction Vector 1");
        assert_eq!(
            state_hash_b2.to_string(),
            "9febb4ee5ce09acf044e8d34238c3e2ec6315382dc1008bc985ac403201b5287"
        );

        let mut block_2 = Block {
            parent_hash: block_hash_1,
            height: 2,
            epoch: 0,
            proposer_id: val_3,
            transactions: vec![signed_tx_1.clone()],
            signatures: vec![],
            state_hash: state_hash_b2,
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_2, &val_1);
        sign_block(&sk_val_2, &mut block_2, &val_2);
        sign_block(&sk_val_3, &mut block_2, &val_3);

        let applied_state_b2 =
            apply_block(&state_after_b1, &block_2, &block_hash_1, 1).expect("Block 2 should apply");
        assert_eq!(compute_state_hash(&applied_state_b2), state_hash_b2);
        assert_eq!(
            applied_state_b2.get_account(&account_a).unwrap().balance,
            1_100_000
        );
        assert_eq!(
            applied_state_b2.get_account(&account_b).unwrap().balance,
            1_000_010
        );
        assert_eq!(
            applied_state_b2.get_account(&account_c).unwrap().balance,
            1_000_010
        );
        assert_eq!(
            applied_state_b2.get_account(&account_d).unwrap().balance,
            900_000
        );
        assert_eq!(applied_state_b2.get_account(&account_d).unwrap().nonce, 1);
        assert_eq!(applied_state_b2.total_supply, 4_000_020);

        let block_hash_2 = compute_block_hash(&block_2);

        let tx_2_invalid_nonce = Transaction {
            sender: account_d,
            recipient: account_a,
            amount: 1,
            nonce: 0,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx_2_invalid_nonce = tx_2_invalid_nonce.clone();
        signed_tx_2_invalid_nonce.signature =
            axiom_crypto::sign_transaction(&sk_val_1, &tx_2_invalid_nonce);

        let mut block_3_invalid_nonce = Block {
            parent_hash: block_hash_2,
            height: 3,
            epoch: 0,
            proposer_id: val_1,
            transactions: vec![signed_tx_2_invalid_nonce],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_3_invalid_nonce, &val_1);
        sign_block(&sk_val_2, &mut block_3_invalid_nonce, &val_2);
        sign_block(&sk_val_3, &mut block_3_invalid_nonce, &val_3);
        let res = apply_block(&state_after_b2, &block_3_invalid_nonce, &block_hash_2, 2);
        assert!(matches!(res, Err(ExecutionError::InvalidNonce { .. })));
        assert_eq!(compute_state_hash(&state_after_b2), state_hash_b2);

        let tx_3_invalid_sig = Transaction {
            sender: account_d,
            recipient: account_a,
            amount: 1_000,
            nonce: 1,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx_3_invalid_sig = tx_3_invalid_sig.clone();
        signed_tx_3_invalid_sig.signature =
            axiom_crypto::sign_transaction(&sk_val_2, &tx_3_invalid_sig);

        let mut block_3_invalid_sig = Block {
            parent_hash: block_hash_2,
            height: 3,
            epoch: 0,
            proposer_id: val_1,
            transactions: vec![signed_tx_3_invalid_sig],
            signatures: vec![],
            state_hash: StateHash([0u8; 32]),
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_3_invalid_sig, &val_1);
        sign_block(&sk_val_2, &mut block_3_invalid_sig, &val_2);
        sign_block(&sk_val_3, &mut block_3_invalid_sig, &val_3);
        let res = apply_block(&state_after_b2, &block_3_invalid_sig, &block_hash_2, 2);
        assert!(matches!(res, Err(ExecutionError::CryptoError(_))));
        assert_eq!(compute_state_hash(&state_after_b2), state_hash_b2);

        let account_e = AccountId(
            axiom_primitives::from_hex(
                "b09bcc8b365f5df9d6829ecfb1aa4b524b723138eacdf002b7e73602f19d9fb0",
            )
            .expect("Invalid account-E hex")
            .try_into()
            .expect("Account-E must be 32 bytes"),
        );

        let tx_4_to_new_account = Transaction {
            sender: account_d,
            recipient: account_e,
            amount: 50_000,
            nonce: 1,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
        };
        let mut signed_tx_4_to_new_account = tx_4_to_new_account.clone();
        signed_tx_4_to_new_account.signature =
            axiom_crypto::sign_transaction(&sk_val_1, &tx_4_to_new_account);

        let (state_after_b3, state_hash_b3) = execute_proposal(
            &state_after_b2,
            std::slice::from_ref(&signed_tx_4_to_new_account),
            &val_1,
        )
        .expect("Failed to execute Transaction Vector 4");
        assert_eq!(
            state_hash_b3.to_string(),
            "d8f1fb0f42dfcb895d87c3c46c8203615061b312123bb4aa9e6c97630af4c181"
        );
        assert_eq!(compute_state_hash(&state_after_b3), state_hash_b3);

        let mut block_3 = Block {
            parent_hash: block_hash_2,
            height: 3,
            epoch: 0,
            proposer_id: val_1,
            transactions: vec![signed_tx_4_to_new_account],
            signatures: vec![],
            state_hash: state_hash_b3,
            timestamp: 0,
        };
        sign_block(&sk_val_1, &mut block_3, &val_1);
        sign_block(&sk_val_2, &mut block_3, &val_2);
        sign_block(&sk_val_3, &mut block_3, &val_3);

        let applied_state_b3 =
            apply_block(&state_after_b2, &block_3, &block_hash_2, 2).expect("Block 3 should apply");
        assert_eq!(compute_state_hash(&applied_state_b3), state_hash_b3);
        assert_eq!(
            applied_state_b3.get_account(&account_d).unwrap().balance,
            850_010
        );
        assert_eq!(applied_state_b3.get_account(&account_d).unwrap().nonce, 2);
        assert_eq!(applied_state_b3.get_account(&account_e).unwrap().balance, 50_000);
        assert_eq!(applied_state_b3.total_supply, 4_000_030);
    }
}
