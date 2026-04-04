#![deny(warnings)]

use axiom_crypto::{compute_block_hash, sign_vote, PrivateKey};
use axiom_execution::{apply_block_v2, execute_proposal_v2, ExecutionError};
use axiom_primitives::{
    Block, BlockHash, ProtocolVersion, Transaction, ValidatorId, ValidatorSignature,
};
use axiom_state::{StakingState, State};
use thiserror::Error;

pub mod bft;

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("Execution error: {0}")]
    Execution(#[from] ExecutionError),
    #[error("No active validators")]
    NoActiveValidators,
    #[error("Crypto error: {0}")]
    Crypto(#[from] axiom_crypto::CryptoError),
    #[error("Arithmetic overflow")]
    Overflow,
    #[error("Proposer mismatch: expected {expected}, got {got}")]
    ProposerMismatch {
        expected: ValidatorId,
        got: ValidatorId,
    },
}

/// Validates and applies a block to the state.
/// This wraps execution::apply_block.
pub fn validate_and_commit_block(
    state: &State,
    staking_state: &StakingState,
    block: &Block,
    parent_hash: &BlockHash,
    parent_height: u64,
) -> Result<(State, StakingState), ConsensusError> {
    let (new_state, new_staking) =
        apply_block_v2(state, staking_state, block, parent_hash, parent_height)?;
    Ok((new_state, new_staking))
}

/// Constructs a new block proposal.
/// - Calculates next height
/// - Sets parent hash
/// - Selects transactions (passed in)
/// - Computes state hash (by executing transactions)
/// - Signs the block (Vote) as the proposer
pub fn construct_block(
    state: &State,
    staking_state: &StakingState,
    height: u64,
    parent_hash: BlockHash,
    transactions: Vec<Transaction>,
    proposer_key: &PrivateKey,
    proposer_id: &ValidatorId,
) -> Result<Block, ConsensusError> {
    // Verify we are a valid active validator
    // We relax the strict rotation check to allow fallback proposers.
    // The node/consensus loop determines WHEN to propose (primary vs fallback).
    match state.get_validator(proposer_id) {
        Some(val) if val.active => {}
        Some(_) => {
            return Err(ConsensusError::Execution(
                ExecutionError::InactiveValidator { id: *proposer_id },
            ))
        }
        None => {
            return Err(ConsensusError::Execution(
                ExecutionError::UnknownValidator { id: *proposer_id },
            ))
        }
    }

    // Execute transactions to get state hash (protocol-aware)
    let (_, _, state_hash) =
        execute_proposal_v2(state, staking_state, &transactions, proposer_id, height)?;

    // Create block template
    let protocol_version = ProtocolVersion::for_height(height).as_u64();
    let mut block = Block {
        parent_hash,
        height,
        epoch: 0, // v1 is always epoch 0
        protocol_version,
        round: 0,
        proposer_id: *proposer_id,
        transactions,
        signatures: vec![],
        state_hash,
        timestamp: 0,
    };

    // Sign the block (Vote)
    // PROTOCOL.md Section 8.4: Vote message = SHA-256(block_hash || height as u64 big-endian)
    let block_hash = compute_block_hash(&block);
    let signature = sign_vote(proposer_key, &block_hash, height);

    block.signatures.push(ValidatorSignature {
        validator_id: *proposer_id,
        signature,
    });

    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_crypto::test_keypair;
    use axiom_execution::select_proposer;
    use axiom_primitives::{
        AccountId, GenesisAccount, GenesisConfig, GenesisValidator, TransactionType,
    };

    fn create_genesis_state() -> (State, axiom_crypto::PrivateKey, ValidatorId, AccountId) {
        let (sk, pk) = test_keypair("val1");
        let val_id = ValidatorId(pk.0);
        let acc_id = AccountId(pk.0);

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

    #[test]
    fn test_construct_and_verify_block() {
        let (state, sk, val_id, _acc_id) = create_genesis_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);
        let height = 1;

        // 1. Construct block
        let block = construct_block(&state, &staking, height, parent_hash, vec![], &sk, &val_id)
            .expect("Failed to construct block");

        let json = serde_json::to_string(&block).unwrap();
        println!("Block JSON: {json}");

        assert_eq!(block.height, 1);
        assert_eq!(block.signatures.len(), 1);
        assert_eq!(block.proposer_id, val_id);

        // 2. Validate block (simulate receiving it)
        // Note: validation requires quorum > 2/3.
        // We have 1 validator with 100 voting power. 1 signature.
        // 100 > 2/3 * 100 (66). So quorum should be met.

        let (new_state, _new_staking) =
            validate_and_commit_block(&state, &staking, &block, &parent_hash, 0)
                .expect("Failed to validate block");

        assert_eq!(new_state.block_reward, 10);
        assert_eq!(new_state.total_supply, 1010);
    }

    #[test]
    fn test_unknown_proposer_cannot_construct() {
        let (state, _, _, _) = create_genesis_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);

        // Create a random key that is NOT the validator
        let (wrong_sk, wrong_pk) = test_keypair("wrong");
        let wrong_id = ValidatorId(wrong_pk.0);

        let res = construct_block(
            &state,
            &staking,
            1,
            parent_hash,
            vec![],
            &wrong_sk,
            &wrong_id,
        );

        assert!(matches!(
            res,
            Err(ConsensusError::Execution(
                ExecutionError::UnknownValidator { .. }
            ))
        ));
    }

    #[test]
    fn test_execution_failure_propagates() {
        let (state, sk, val_id, acc_id) = create_genesis_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);

        // Create invalid transaction (insufficient balance)
        let tx = Transaction {
            sender: acc_id,
            recipient: acc_id,
            amount: 2000, // Balance is 1000
            nonce: 0,
            signature: axiom_primitives::Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
            evidence: None,
        };
        let mut signed_tx = tx.clone();
        signed_tx.signature = axiom_crypto::sign_transaction(&sk, &tx);

        let res = construct_block(
            &state,
            &staking,
            1,
            parent_hash,
            vec![signed_tx],
            &sk,
            &val_id,
        );

        // Should fail with Execution error inside Consensus error
        match res {
            Err(ConsensusError::Execution(ExecutionError::InsufficientBalance { .. })) => (),
            _ => panic!("Expected Execution(InsufficientBalance), got {res:?}"),
        }
    }

    // Helper to create state with 3 validators
    fn create_3_validators_state() -> (State, Vec<(axiom_crypto::PrivateKey, ValidatorId)>) {
        let (sk1, pk1) = test_keypair("val1");
        let (sk2, pk2) = test_keypair("val2");
        let (sk3, pk3) = test_keypair("val3");

        let val1_id = ValidatorId(pk1.0);
        let val2_id = ValidatorId(pk2.0);
        let val3_id = ValidatorId(pk3.0);

        let acc1_id = AccountId(pk1.0);
        let acc2_id = AccountId(pk2.0);
        let acc3_id = AccountId(pk3.0);

        let genesis = GenesisConfig {
            total_supply: 3000,
            block_reward: 10,
            accounts: vec![
                GenesisAccount {
                    id: acc1_id,
                    balance: 1000,
                    nonce: 0,
                },
                GenesisAccount {
                    id: acc2_id,
                    balance: 1000,
                    nonce: 0,
                },
                GenesisAccount {
                    id: acc3_id,
                    balance: 1000,
                    nonce: 0,
                },
            ],
            validators: vec![
                GenesisValidator {
                    id: val1_id,
                    voting_power: 10,
                    account_id: acc1_id,
                    active: true,
                },
                GenesisValidator {
                    id: val2_id,
                    voting_power: 10,
                    account_id: acc2_id,
                    active: true,
                },
                GenesisValidator {
                    id: val3_id,
                    voting_power: 10,
                    account_id: acc3_id,
                    active: true,
                },
            ],
        };

        let state = State::from_genesis(&genesis).unwrap();
        let validators = vec![(sk1, val1_id), (sk2, val2_id), (sk3, val3_id)];

        (state, validators)
    }

    #[test]
    fn test_quorum_boundary() {
        let (state, validators) = create_3_validators_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);
        let height = 1;

        // Use validator 1 as proposer (sorted: val1, val2, val3 depending on hash)
        // We just need a valid block first.
        let proposer_id = select_proposer(&state, height).unwrap();
        let (proposer_sk, _) = validators
            .iter()
            .find(|(_, id)| *id == proposer_id)
            .unwrap();

        // Construct base block with 1 signature (proposer's)
        let mut block = construct_block(
            &state,
            &staking,
            height,
            parent_hash,
            vec![],
            proposer_sk,
            &proposer_id,
        )
        .unwrap();

        // Current power: 10. Total: 30. Required: > 20 (i.e. 21+).
        // 1 signature = 10 power. Should fail.
        let res = validate_and_commit_block(&state, &staking, &block, &parent_hash, 0);
        assert!(matches!(
            res,
            Err(ConsensusError::Execution(
                ExecutionError::QuorumNotMet { .. }
            ))
        ));

        // Add 2nd signature. Total power: 20. Required: > 20.
        // 20 is NOT > 20. Should fail.
        // Find a validator that is not the proposer
        let (val2_sk, val2_id) = validators
            .iter()
            .find(|(_, id)| *id != proposer_id)
            .unwrap();

        let block_hash = compute_block_hash(&block);
        let sig2 = sign_vote(val2_sk, &block_hash, height);
        block.signatures.push(ValidatorSignature {
            validator_id: *val2_id,
            signature: sig2,
        });

        let res = validate_and_commit_block(&state, &staking, &block, &parent_hash, 0);
        assert!(matches!(
            res,
            Err(ConsensusError::Execution(
                ExecutionError::QuorumNotMet { .. }
            ))
        ));

        // Add 3rd signature. Total power: 30. Required: > 20.
        // 30 > 20. Should pass.
        let (val3_sk, val3_id) = validators
            .iter()
            .find(|(_, id)| *id != proposer_id && *id != *val2_id)
            .unwrap();
        let sig3 = sign_vote(val3_sk, &block_hash, height);
        block.signatures.push(ValidatorSignature {
            validator_id: *val3_id,
            signature: sig3,
        });

        validate_and_commit_block(&state, &staking, &block, &parent_hash, 0)
            .expect("3/3 should pass");
    }

    #[test]
    fn test_unknown_validator_signature() {
        let (state, sk, val_id, _) = create_genesis_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);

        let mut block =
            construct_block(&state, &staking, 1, parent_hash, vec![], &sk, &val_id).unwrap();

        // Add signature from unknown validator
        let (unknown_sk, unknown_pk) = test_keypair("unknown");
        let unknown_id = ValidatorId(unknown_pk.0);
        let block_hash = compute_block_hash(&block);
        let sig = sign_vote(&unknown_sk, &block_hash, 1);

        block.signatures.push(ValidatorSignature {
            validator_id: unknown_id,
            signature: sig,
        });

        let res = validate_and_commit_block(&state, &staking, &block, &parent_hash, 0);
        assert!(matches!(
            res,
            Err(ConsensusError::Execution(
                ExecutionError::UnknownValidator { .. }
            ))
        ));
    }

    #[test]
    fn test_duplicate_signature() {
        let (state, sk, val_id, _) = create_genesis_state();
        let staking = StakingState::empty();
        let parent_hash = BlockHash([0u8; 32]);

        let mut block =
            construct_block(&state, &staking, 1, parent_hash, vec![], &sk, &val_id).unwrap();

        // Duplicate the existing signature
        let sig = block.signatures[0].clone();
        block.signatures.push(sig);

        let res = validate_and_commit_block(&state, &staking, &block, &parent_hash, 0);
        assert!(matches!(
            res,
            Err(ConsensusError::Execution(
                ExecutionError::DuplicateSignature { .. }
            ))
        ));
    }

    #[test]
    fn test_proposer_rotation() {
        let (state, validators) = create_3_validators_state();

        // Sort validators by ID to match state's internal BTreeMap order
        let mut sorted_vals = validators.clone();
        sorted_vals.sort_by(|a, b| a.1.cmp(&b.1));

        // Height 1: Index 1 % 3 = 1
        let p1 = select_proposer(&state, 1).unwrap();
        assert_eq!(p1, sorted_vals[1].1);

        // Height 2: Index 2 % 3 = 2
        let p2 = select_proposer(&state, 2).unwrap();
        assert_eq!(p2, sorted_vals[2].1);

        // Height 3: Index 3 % 3 = 0
        let p3 = select_proposer(&state, 3).unwrap();
        assert_eq!(p3, sorted_vals[0].1);

        // Height 4: Index 4 % 3 = 1
        let p4 = select_proposer(&state, 4).unwrap();
        assert_eq!(p4, sorted_vals[1].1);
    }

    #[test]
    fn test_empty_validator_set_error() {
        // Create a state with NO validators
        let genesis = GenesisConfig {
            total_supply: 0,
            block_reward: 0,
            accounts: vec![],
            validators: vec![],
        };
        // State::from_genesis might fail or succeed depending on implementation.
        // If it succeeds, select_proposer should fail.
        if let Ok(state) = State::from_genesis(&genesis) {
            let res = select_proposer(&state, 1);
            assert!(matches!(res, Err(ExecutionError::NoActiveValidators)));
        }
    }
}
