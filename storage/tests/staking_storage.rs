#![deny(warnings)]

use axiom_primitives::{
    AccountId, Block, BlockHash, Signature, StakeAmount, StateHash, UnbondingEntry, ValidatorId,
    ValidatorSignature, MIN_VALIDATOR_STAKE, UNBONDING_PERIOD,
};
use axiom_state::{Account, StakingState, State, Validator};
use axiom_storage::Storage;
use std::collections::BTreeMap;
use tempfile::TempDir;

#[test]
fn test_staking_state_storage_roundtrip() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("staking_test.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let acc_id = AccountId([1u8; 32]);
    let val_id = ValidatorId([1u8; 32]);
    let val_id2 = ValidatorId([2u8; 32]);
    let acc_id2 = AccountId([2u8; 32]);

    let mut accounts = BTreeMap::new();
    accounts.insert(
        acc_id,
        Account {
            balance: 700_000,
            nonce: 1,
        },
    );
    accounts.insert(
        acc_id2,
        Account {
            balance: 100_000,
            nonce: 0,
        },
    );

    let mut validators = BTreeMap::new();
    validators.insert(
        val_id,
        Validator {
            voting_power: 100,
            account_id: acc_id,
            active: true,
        },
    );
    validators.insert(
        val_id2,
        Validator {
            voting_power: 50,
            account_id: acc_id2,
            active: true,
        },
    );

    let state = State {
        total_supply: 1_000_000,
        block_reward: 10,
        accounts,
        validators,
    };

    let genesis_hash = StateHash([0u8; 32]);
    storage.store_genesis(&state, &genesis_hash).unwrap();

    let mut staking = StakingState::new_active();
    staking.stakes.insert(val_id, StakeAmount(150_000));
    staking.unbonding_queue.push(UnbondingEntry {
        validator_id: val_id2,
        amount: StakeAmount(50_000),
        release_height: 11_000,
    });

    let block = Block {
        parent_hash: BlockHash([0u8; 32]),
        height: 10_000,
        epoch: 0,
        protocol_version: axiom_primitives::PROTOCOL_VERSION_V2,
        round: 0,
        proposer_id: val_id,
        transactions: vec![],
        signatures: vec![ValidatorSignature {
            validator_id: val_id,
            signature: Signature([0u8; 64]),
        }],
        state_hash: StateHash([0xaa; 32]),
        timestamp: 0,
    };

    storage.commit_block_v2(&block, &state, &staking).unwrap();

    let loaded_staking = storage.load_staking_state().unwrap();
    assert_eq!(loaded_staking.stakes.len(), staking.stakes.len());
    assert_eq!(
        loaded_staking.stakes.get(&val_id).unwrap().0,
        staking.stakes.get(&val_id).unwrap().0,
    );
    assert_eq!(
        loaded_staking.unbonding_queue.len(),
        staking.unbonding_queue.len()
    );
    assert_eq!(
        loaded_staking.unbonding_queue[0].validator_id,
        staking.unbonding_queue[0].validator_id,
    );
    assert_eq!(
        loaded_staking.unbonding_queue[0].amount.0,
        staking.unbonding_queue[0].amount.0,
    );
    assert_eq!(
        loaded_staking.unbonding_queue[0].release_height,
        staking.unbonding_queue[0].release_height,
    );
    assert_eq!(loaded_staking.minimum_stake, MIN_VALIDATOR_STAKE);
    assert_eq!(loaded_staking.unbonding_period, UNBONDING_PERIOD);
}
