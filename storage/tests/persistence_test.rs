use axiom_crypto::compute_block_hash;
use axiom_primitives::{Block, BlockHash, StateHash, ValidatorId};
use axiom_state::State;
use axiom_storage::Storage;
use std::collections::BTreeMap;

// Helper to create a dummy 32-byte hash
fn dummy_hash(b: u8) -> BlockHash {
    BlockHash([b; 32])
}

// Helper to create a dummy state hash
fn dummy_state_hash(b: u8) -> StateHash {
    StateHash([b; 32])
}

// Helper to create a dummy validator ID
fn dummy_validator_id(b: u8) -> ValidatorId {
    ValidatorId([b; 32])
}

#[test]
fn test_persistence_and_restart() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");

    // 1. Initialize Storage
    let storage = Storage::initialize(&db_path).unwrap();

    // 2. Create Dummy State
    let original_state = State {
        accounts: BTreeMap::new(),
        validators: BTreeMap::new(),
        total_supply: 1_000_000,
        block_reward: 10,
    };

    let height = 100;
    let state_hash = dummy_state_hash(0xAA);

    // 3. Store Genesis (Simulate initial state)
    // In this test we just want to test commit_block, but let's store genesis first to be clean
    // or just commit_block directly (which upserts).
    // Let's use store_genesis to set initial values if needed, but here we can just commit.
    // Actually, store_genesis sets "genesis_hash" meta.
    storage.store_genesis(&original_state, &state_hash).unwrap();

    // 4. Create Dummy Block
    let block = Block {
        parent_hash: dummy_hash(0x99),
        height,
        epoch: 5,
        proposer_id: dummy_validator_id(1),
        transactions: vec![],
        signatures: vec![],
        state_hash,
        timestamp: 0,
    };

    // 5. Commit Block (Updates State to original_state)
    storage.commit_block(&block, &original_state).unwrap();
    let expected_block_hash = compute_block_hash(&block);

    // 6. Simulate Restart (New Storage Instance)
    drop(storage);
    let storage_2 = Storage::initialize(&db_path).unwrap();

    // 7. Load State (Latest)
    let loaded_state = storage_2.load_latest_state().unwrap().unwrap().0;
    let loaded_height = storage_2.get_latest_height().unwrap();

    // 8. Verify State
    assert_eq!(loaded_height, height);
    assert_eq!(loaded_state.total_supply, original_state.total_supply);
    assert_eq!(loaded_state.block_reward, original_state.block_reward);

    // 9. Load Block
    let loaded_block = storage_2
        .get_block_by_height(height)
        .unwrap()
        .expect("Block should exist");

    // 10. Verify Block
    assert_eq!(loaded_block.height, height);
    assert_eq!(loaded_block.parent_hash, dummy_hash(0x99));
    assert_eq!(loaded_block.state_hash, state_hash);

    // Verify block hash matches
    assert_eq!(compute_block_hash(&loaded_block), expected_block_hash);
}
