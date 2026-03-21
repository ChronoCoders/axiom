use axiom_crypto::{compute_block_hash, sign_consensus_vote, sign_transaction_for_height, test_keypair};
use axiom_execution::{execute_proposal_v2, select_proposer_v2};
use axiom_primitives::{
    AccountId, Block, BlockHash, Evidence, ProtocolVersion, Signature, Transaction, TransactionType,
    ValidatorId, Vote, VotePhase, V2_ACTIVATION_HEIGHT,
};
use axiom_state::{StakingState, State};

fn load_reference_genesis_state() -> State {
    let genesis = axiom_primitives::deserialize_genesis_json(include_str!("../../fixtures/reference_genesis.json")).unwrap();
    State::from_genesis(&genesis).unwrap()
}

fn mk_vote(
    validator_id: ValidatorId,
    height: u64,
    round: u64,
    phase: VotePhase,
    block_hash: BlockHash,
    signature: Signature,
) -> Vote {
    Vote {
        height,
        round,
        phase,
        block_hash: Some(block_hash),
        validator_id,
        signature,
    }
}

#[test]
fn test_v2_locked_vectors() {
    let (sk1, pk1) = test_keypair("axiom-test-validator-1");
    let (sk2, pk2) = test_keypair("axiom-test-validator-2");
    let (sk3, pk3) = test_keypair("axiom-test-validator-3");

    let v1 = ValidatorId(pk1.0);
    let v2 = ValidatorId(pk2.0);
    let v3 = ValidatorId(pk3.0);

    let mut state = load_reference_genesis_state();
    let mut staking = StakingState::empty();

    let mut parent = BlockHash([0u8; 32]);

    let height_0 = V2_ACTIVATION_HEIGHT;
    let proposer_0 = select_proposer_v2(&state, &staking, height_0, 0).unwrap();
    let (state_0, staking_0, state_hash_0) =
        execute_proposal_v2(&state, &staking, &[], &proposer_0, height_0).unwrap();
    let block_0 = Block {
        parent_hash: parent,
        height: height_0,
        epoch: 0,
        protocol_version: ProtocolVersion::V2.as_u64(),
        round: 0,
        proposer_id: proposer_0,
        transactions: vec![],
        signatures: vec![],
        state_hash: state_hash_0,
        timestamp: 0,
    };
    let block_hash_0 = compute_block_hash(&block_0);
    assert_eq!(
        proposer_0.to_string(),
        "97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3"
    );
    assert_eq!(
        state_hash_0.to_string(),
        "70fe2678ea11c47825a2eba0ecc0c36112e1cac651c69b0f73ff197dc2a73e76"
    );
    assert_eq!(
        block_hash_0.to_string(),
        "c0fe2b939193dc3d059b303f64e8cde8d109bb7a220a2208db2932f747bed341"
    );

    state = state_0;
    staking = staking_0;
    parent = block_hash_0;

    let height_1 = V2_ACTIVATION_HEIGHT + 1;
    let proposer_1 = select_proposer_v2(&state, &staking, height_1, 0).unwrap();

    let sender_1 = AccountId(v2.0);
    let nonce_1 = state.get_account(&sender_1).unwrap().nonce;
    let tx_1_unsigned = Transaction {
        sender: sender_1,
        recipient: sender_1,
        amount: 1,
        nonce: nonce_1,
        signature: Signature([0u8; 64]),
        tx_type: TransactionType::Unstake,
        evidence: None,
    };
    let tx_1 = Transaction {
        signature: sign_transaction_for_height(height_1, &sk2, &tx_1_unsigned),
        ..tx_1_unsigned
    };
    let txs_1 = vec![tx_1];

    let (state_1, staking_1, state_hash_1) =
        execute_proposal_v2(&state, &staking, &txs_1, &proposer_1, height_1).unwrap();
    let block_1 = Block {
        parent_hash: parent,
        height: height_1,
        epoch: 0,
        protocol_version: ProtocolVersion::V2.as_u64(),
        round: 0,
        proposer_id: proposer_1,
        transactions: txs_1,
        signatures: vec![],
        state_hash: state_hash_1,
        timestamp: 0,
    };
    let block_hash_1 = compute_block_hash(&block_1);
    assert_eq!(
        proposer_1.to_string(),
        "b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf"
    );
    assert_eq!(
        state_hash_1.to_string(),
        "6d2c245f9680e445033da3ca1e578abc24f073615c6574654c9dcb6c6f4341ae"
    );
    assert_eq!(
        block_hash_1.to_string(),
        "4836e851c95e125214d1150234ab761fafbaaecee43ff80b8af7b09224587b4d"
    );
    assert_eq!(staking_1.epoch, 1);

    state = state_1;
    staking = staking_1;
    parent = block_hash_1;

    let height_2 = V2_ACTIVATION_HEIGHT + 2;
    let proposer_2 = select_proposer_v2(&state, &staking, height_2, 0).unwrap();

    let sender_2 = AccountId(v2.0);
    let nonce_2 = state.get_account(&sender_2).unwrap().nonce;
    let tx_2_unsigned = Transaction {
        sender: sender_2,
        recipient: sender_2,
        amount: 1,
        nonce: nonce_2,
        signature: Signature([0u8; 64]),
        tx_type: TransactionType::Stake,
        evidence: None,
    };
    let tx_2 = Transaction {
        signature: sign_transaction_for_height(height_2, &sk2, &tx_2_unsigned),
        ..tx_2_unsigned
    };
    let txs_2 = vec![tx_2];

    let (state_2, staking_2, state_hash_2) =
        execute_proposal_v2(&state, &staking, &txs_2, &proposer_2, height_2).unwrap();
    let block_2 = Block {
        parent_hash: parent,
        height: height_2,
        epoch: staking.epoch,
        protocol_version: ProtocolVersion::V2.as_u64(),
        round: 0,
        proposer_id: proposer_2,
        transactions: txs_2,
        signatures: vec![],
        state_hash: state_hash_2,
        timestamp: 0,
    };
    let block_hash_2 = compute_block_hash(&block_2);
    assert_eq!(
        proposer_2.to_string(),
        "b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf"
    );
    assert_eq!(
        state_hash_2.to_string(),
        "802cde7095913aeed3ace017f47fa38a6dadeb67609fbbe24c753f124ef065ad"
    );
    assert_eq!(
        block_hash_2.to_string(),
        "3c7cb8167cbcfa1b412f44cb0e975e5d0dfcf708ef87c7def19dfca3a7515ce4"
    );
    assert_eq!(staking_2.epoch, 2);

    state = state_2;
    staking = staking_2;
    parent = block_hash_2;

    let height_3 = V2_ACTIVATION_HEIGHT + 3;
    let proposer_3 = select_proposer_v2(&state, &staking, height_3, 0).unwrap();

    let offender_vote_a = mk_vote(
        v3,
        height_1,
        0,
        VotePhase::Prevote,
        BlockHash([0x11u8; 32]),
        Signature([0u8; 64]),
    );
    let offender_vote_a = Vote {
        signature: sign_consensus_vote(&sk3, &offender_vote_a),
        ..offender_vote_a
    };
    let offender_vote_b = mk_vote(
        v3,
        height_1,
        0,
        VotePhase::Prevote,
        BlockHash([0x22u8; 32]),
        Signature([0u8; 64]),
    );
    let offender_vote_b = Vote {
        signature: sign_consensus_vote(&sk3, &offender_vote_b),
        ..offender_vote_b
    };

    let evidence = Evidence::DoubleVote {
        vote_a: Box::new(offender_vote_a),
        vote_b: Box::new(offender_vote_b),
    };

    let sender_3 = AccountId(v1.0);
    let nonce_3 = state.get_account(&sender_3).unwrap().nonce;
    let tx_3_unsigned = Transaction {
        sender: sender_3,
        recipient: sender_3,
        amount: 0,
        nonce: nonce_3,
        signature: Signature([0u8; 64]),
        tx_type: TransactionType::SlashEvidence,
        evidence: Some(evidence),
    };
    let tx_3 = Transaction {
        signature: sign_transaction_for_height(height_3, &sk1, &tx_3_unsigned),
        ..tx_3_unsigned
    };
    let txs_3 = vec![tx_3];

    let (state_3, staking_3, state_hash_3) =
        execute_proposal_v2(&state, &staking, &txs_3, &proposer_3, height_3).unwrap();
    let block_3 = Block {
        parent_hash: parent,
        height: height_3,
        epoch: staking.epoch,
        protocol_version: ProtocolVersion::V2.as_u64(),
        round: 0,
        proposer_id: proposer_3,
        transactions: txs_3,
        signatures: vec![],
        state_hash: state_hash_3,
        timestamp: 0,
    };
    let block_hash_3 = compute_block_hash(&block_3);
    assert_eq!(
        proposer_3.to_string(),
        "b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf"
    );
    assert_eq!(
        state_hash_3.to_string(),
        "fe30df7f408084c2c41127135868ab785a44852d9742534b3add261fd27cbf35"
    );
    assert_eq!(
        block_hash_3.to_string(),
        "31d86321bd12d71f5ebd9dd5cea159c842ae9d711a919429f22c160be3e65067"
    );
    assert_eq!(staking_3.epoch, 3);

    let _ = (state_3, staking_3);
}

