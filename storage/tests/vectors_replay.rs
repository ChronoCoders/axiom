use axiom_crypto::{compute_block_hash, sign_transaction, sign_vote, test_keypair, PrivateKey};
use axiom_execution::{apply_block, compute_state_hash, execute_proposal};
use axiom_primitives::{
    AccountId, Block, BlockHash, Signature, StateHash, Transaction, TransactionType, ValidatorId,
    ValidatorSignature,
};
use axiom_state::{Account, State, Validator};
use axiom_storage::Storage;
use std::collections::BTreeMap;

#[allow(dead_code)]
struct TestContext {
    val1_sk: PrivateKey,
    val1_id: ValidatorId,
    val2_sk: PrivateKey,
    val2_id: ValidatorId,
    val3_sk: PrivateKey,
    val3_id: ValidatorId,
    acc_a_sk: PrivateKey,
    acc_a_id: AccountId,
    acc_b_sk: PrivateKey,
    acc_b_id: AccountId,
    initial_state: State,
}

fn setup_context() -> TestContext {
    let (val1_sk, val1_pk) = test_keypair("val1");
    let val1_id = ValidatorId(val1_pk.0);

    let (val2_sk, val2_pk) = test_keypair("val2");
    let val2_id = ValidatorId(val2_pk.0);

    let (val3_sk, val3_pk) = test_keypair("val3");
    let val3_id = ValidatorId(val3_pk.0);

    let (acc_a_sk, acc_a_pk) = test_keypair("acc_a");
    let acc_a_id = AccountId(acc_a_pk.0);

    let (acc_b_sk, acc_b_pk) = test_keypair("acc_b");
    let acc_b_id = AccountId(acc_b_pk.0);

    let mut state = State {
        total_supply: 0,
        block_reward: 10,
        accounts: BTreeMap::new(),
        validators: BTreeMap::new(),
    };

    // Voting powers: Val1=2, Val2=2, Val3=1. Total=5. 2/3=3.33 -> Quorum > 3.33 => 4.
    // Val1+Val2 = 4. OK.

    // Val 1 -> Account A
    state.validators.insert(
        val1_id,
        Validator {
            voting_power: 2,
            account_id: acc_a_id,
            active: true,
        },
    );

    // Val 2 -> Account B
    state.validators.insert(
        val2_id,
        Validator {
            voting_power: 2,
            account_id: acc_b_id,
            active: true,
        },
    );

    // Val 3 -> Account A
    state.validators.insert(
        val3_id,
        Validator {
            voting_power: 1,
            account_id: acc_a_id,
            active: true,
        },
    );

    // Account A
    state.accounts.insert(
        acc_a_id,
        Account {
            balance: 1000,
            nonce: 0,
        },
    );

    // Account B
    state.accounts.insert(
        acc_b_id,
        Account {
            balance: 1000,
            nonce: 0,
        },
    );

    state.total_supply = 2000;

    TestContext {
        val1_sk,
        val1_id,
        val2_sk,
        val2_id,
        val3_sk,
        val3_id,
        acc_a_sk,
        acc_a_id,
        acc_b_sk,
        acc_b_id,
        initial_state: state,
    }
}

fn create_and_sign_block(
    state: &State,
    parent_hash: BlockHash,
    height: u64,
    proposer_id: ValidatorId,
    transactions: Vec<Transaction>,
    signers: Vec<(&ValidatorId, &PrivateKey)>,
) -> Block {
    let mut block = Block {
        parent_hash,
        height,
        epoch: 0,
        protocol_version: axiom_primitives::PROTOCOL_VERSION_V1,
        round: 0,
        proposer_id,
        transactions: transactions.clone(),
        signatures: vec![],
        state_hash: StateHash([0; 32]), // Placeholder
        timestamp: 0,
    };

    // 1. Compute state hash
    let (_, state_hash) =
        execute_proposal(state, &transactions, &proposer_id).expect("Failed to execute proposal");
    block.state_hash = state_hash;

    // 2. Compute block hash
    let block_hash = compute_block_hash(&block);

    // 3. Sign
    for (vid, sk) in signers {
        let sig = sign_vote(sk, &block_hash, height);
        block.signatures.push(ValidatorSignature {
            validator_id: *vid,
            signature: sig,
        });
    }

    block
}

#[test]
fn test_vector_9_replay_test() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("replay.db");
    let storage = Storage::initialize(&db_path).unwrap();

    let ctx = setup_context();
    let mut state = ctx.initial_state.clone();
    let genesis_hash = BlockHash([0; 32]);

    let mut current_block_hash = genesis_hash;
    let mut current_height = 0;

    // -------------------------------------------------------------------------
    // Create Block 1 (Empty)
    // -------------------------------------------------------------------------
    let block_1 = create_and_sign_block(
        &state,
        current_block_hash,
        1,
        ctx.val1_id,
        vec![],
        vec![(&ctx.val1_id, &ctx.val1_sk), (&ctx.val2_id, &ctx.val2_sk)],
    );

    state = apply_block(&state, &block_1, &current_block_hash, current_height)
        .expect("Failed to apply block 1");

    storage.commit_block(&block_1, &state).unwrap();

    current_block_hash = compute_block_hash(&block_1);
    current_height = 1;

    // -------------------------------------------------------------------------
    // Create Block 2 (With Transaction)
    // -------------------------------------------------------------------------
    let mut tx = Transaction {
        sender: ctx.acc_a_id,
        recipient: ctx.acc_b_id,
        amount: 10,
        nonce: 0,                      // Account A starts with nonce 0.
        signature: Signature([0; 64]), // Placeholder
        tx_type: TransactionType::Transfer,
    };
    tx.signature = sign_transaction(&ctx.acc_a_sk, &tx);

    let block_2 = create_and_sign_block(
        &state,
        current_block_hash,
        2,
        ctx.val2_id,
        vec![tx],
        vec![(&ctx.val1_id, &ctx.val1_sk), (&ctx.val2_id, &ctx.val2_sk)],
    );

    state = apply_block(&state, &block_2, &current_block_hash, current_height)
        .expect("Failed to apply block 2");
    storage.commit_block(&block_2, &state).unwrap();

    let expected_final_hash = block_2.state_hash;

    // -------------------------------------------------------------------------
    // Replay
    // -------------------------------------------------------------------------
    let mut replay_state = ctx.initial_state.clone();
    let mut replay_block_hash = genesis_hash;
    let mut replay_height = 0;

    let loaded_block_1 = storage
        .get_block_by_height(1)
        .unwrap()
        .expect("Block 1 missing");
    replay_state = apply_block(
        &replay_state,
        &loaded_block_1,
        &replay_block_hash,
        replay_height,
    )
    .unwrap();
    replay_block_hash = compute_block_hash(&loaded_block_1);
    replay_height = 1;

    let loaded_block_2 = storage
        .get_block_by_height(2)
        .unwrap()
        .expect("Block 2 missing");
    replay_state = apply_block(
        &replay_state,
        &loaded_block_2,
        &replay_block_hash,
        replay_height,
    )
    .unwrap();

    assert_eq!(compute_state_hash(&replay_state), expected_final_hash);
}

#[test]
fn test_vector_10_determinism_test() {
    let temp_dir_1 = tempfile::tempdir().unwrap();
    let db_path_1 = temp_dir_1.path().join("node1.db");
    let storage_1 = Storage::initialize(&db_path_1).unwrap();

    let temp_dir_2 = tempfile::tempdir().unwrap();
    let db_path_2 = temp_dir_2.path().join("node2.db");
    let storage_2 = Storage::initialize(&db_path_2).unwrap();

    let ctx = setup_context();
    let mut state_1 = ctx.initial_state.clone();
    let mut state_2 = ctx.initial_state.clone();
    let genesis_hash = BlockHash([0; 32]);

    let mut current_hash_1 = genesis_hash;
    let mut current_height_1 = 0;

    let mut current_hash_2 = genesis_hash;
    let mut current_height_2 = 0;

    // Store genesis state so load_latest_state works
    storage_1
        .store_genesis(&ctx.initial_state, &StateHash([0; 32]))
        .unwrap();
    storage_2
        .store_genesis(&ctx.initial_state, &StateHash([0; 32]))
        .unwrap();

    // -------------------------------------------------------------------------
    // Block 1
    // -------------------------------------------------------------------------
    let block_1 = create_and_sign_block(
        &state_1, // state is same for both
        genesis_hash,
        1,
        ctx.val1_id,
        vec![],
        vec![(&ctx.val1_id, &ctx.val1_sk), (&ctx.val2_id, &ctx.val2_sk)],
    );

    // Node 1
    state_1 = apply_block(&state_1, &block_1, &current_hash_1, current_height_1).unwrap();
    storage_1.commit_block(&block_1, &state_1).unwrap();
    current_hash_1 = compute_block_hash(&block_1);
    current_height_1 = 1;

    // Node 2
    state_2 = apply_block(&state_2, &block_1, &current_hash_2, current_height_2).unwrap();
    storage_2.commit_block(&block_1, &state_2).unwrap();
    current_hash_2 = compute_block_hash(&block_1);
    current_height_2 = 1;

    // -------------------------------------------------------------------------
    // Block 2
    // -------------------------------------------------------------------------
    let mut tx = Transaction {
        sender: ctx.acc_a_id,
        recipient: ctx.acc_b_id,
        amount: 10,
        nonce: 0,
        signature: Signature([0; 64]),
        tx_type: TransactionType::Transfer,
    };
    tx.signature = sign_transaction(&ctx.acc_a_sk, &tx);

    let block_2 = create_and_sign_block(
        &state_1,
        current_hash_1,
        2,
        ctx.val2_id,
        vec![tx],
        vec![(&ctx.val1_id, &ctx.val1_sk), (&ctx.val2_id, &ctx.val2_sk)],
    );

    // Node 1
    state_1 = apply_block(&state_1, &block_2, &current_hash_1, current_height_1).unwrap();
    storage_1.commit_block(&block_2, &state_1).unwrap();

    // Node 2
    state_2 = apply_block(&state_2, &block_2, &current_hash_2, current_height_2).unwrap();
    storage_2.commit_block(&block_2, &state_2).unwrap();

    // 10.2 Expected Result
    assert_eq!(compute_state_hash(&state_1), compute_state_hash(&state_2));

    // Verify loaded states
    let loaded_state_1 = storage_1.load_latest_state().unwrap().unwrap().0;
    let loaded_state_2 = storage_2.load_latest_state().unwrap().unwrap().0;
    assert_eq!(
        compute_state_hash(&loaded_state_1),
        compute_state_hash(&loaded_state_2)
    );
}
