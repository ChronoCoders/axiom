#![deny(warnings)]

use axiom_consensus::bft::{Engine, Outbound, ProposalInputs, Step};
use axiom_crypto::{compute_block_hash, test_keypair, verify_precommit, PrivateKey};
use axiom_primitives::{
    AccountId, BlockHash, GenesisAccount, GenesisConfig, GenesisValidator, LockState,
    ProtocolVersion, ValidatorId, VotePhase, V2_ACTIVATION_HEIGHT,
};
use axiom_state::{StakingState, State};
use std::collections::BTreeMap;

fn mk_state_and_staking() -> (State, StakingState, Vec<(ValidatorId, PrivateKey)>) {
    let (sk1, pk1) = test_keypair("bft_val_1");
    let (sk2, pk2) = test_keypair("bft_val_2");
    let (sk3, pk3) = test_keypair("bft_val_3");

    let v1 = ValidatorId(pk1.0);
    let v2 = ValidatorId(pk2.0);
    let v3 = ValidatorId(pk3.0);

    let a1 = AccountId(pk1.0);
    let a2 = AccountId(pk2.0);
    let a3 = AccountId(pk3.0);

    let total_supply = 2_600_000;
    let genesis = GenesisConfig {
        total_supply,
        block_reward: 0,
        accounts: vec![
            GenesisAccount {
                id: a1,
                balance: 850_000,
                nonce: 0,
            },
            GenesisAccount {
                id: a2,
                balance: 850_000,
                nonce: 0,
            },
            GenesisAccount {
                id: a3,
                balance: 900_000,
                nonce: 0,
            },
        ],
        validators: vec![
            GenesisValidator {
                id: v1,
                voting_power: 1,
                account_id: a1,
                active: true,
            },
            GenesisValidator {
                id: v2,
                voting_power: 1,
                account_id: a2,
                active: true,
            },
            GenesisValidator {
                id: v3,
                voting_power: 1,
                account_id: a3,
                active: true,
            },
        ],
    };

    let mut state = State::from_genesis(&genesis).unwrap();
    state.total_supply = 3_000_000;

    let mut stakes = BTreeMap::new();
    stakes.insert(v1, axiom_primitives::StakeAmount(150_000));
    stakes.insert(v2, axiom_primitives::StakeAmount(150_000));
    stakes.insert(v3, axiom_primitives::StakeAmount(100_000));

    let staking = StakingState {
        stakes,
        minimum_stake: axiom_primitives::MIN_VALIDATOR_STAKE,
        unbonding_period: axiom_primitives::UNBONDING_PERIOD,
        unbonding_queue: Vec::new(),
        epoch: 0,
        jailed_validators: std::collections::BTreeSet::new(),
        processed_evidence: std::collections::BTreeSet::new(),
    };

    (state, staking, vec![(v1, sk1), (v2, sk2), (v3, sk3)])
}

#[test]
fn test_v2_bft_commit_flow_and_signatures() {
    let (state, staking, keys) = mk_state_and_staking();
    let height = V2_ACTIVATION_HEIGHT + 1;
    assert_eq!(ProtocolVersion::for_height(height), ProtocolVersion::V2);

    let mut engines: Vec<(ValidatorId, Engine)> = keys
        .iter()
        .map(|(vid, _)| {
            (
                *vid,
                Engine::new(
                    height,
                    LockState {
                        height,
                        round: 0,
                        block_hash: None,
                    },
                ),
            )
        })
        .collect();

    let proposer = engines[0].1.proposer_for_round(&state, &staking).unwrap();
    let (proposer_id, proposer_sk) = keys.iter().find(|(v, _)| *v == proposer).unwrap();

    let parent_hash = BlockHash([0u8; 32]);
    let txs = Vec::new();

    let mut proposal_msg = None;
    for (vid, engine) in engines.iter_mut() {
        if *vid == proposer {
            let out = engine
                .make_proposal(ProposalInputs {
                    state: &state,
                    staking: &staking,
                    parent_hash,
                    transactions: txs.clone(),
                    proposer_key: proposer_sk,
                    proposer_id,
                    timestamp: 0,
                })
                .unwrap();
            match out {
                Outbound::Proposal(p) => proposal_msg = Some(p),
                _ => unreachable!(),
            }
        }
    }
    let proposal = proposal_msg.unwrap();
    let block_hash = compute_block_hash(&proposal.block);

    for (_, engine) in engines.iter_mut() {
        let _ = engine
            .on_proposal(&state, &staking, proposal.clone())
            .unwrap();
        assert_eq!(engine.step, Step::Prevote);
    }

    let mut prevotes = Vec::new();
    for (vid, engine) in engines.iter_mut() {
        let (_, sk) = keys.iter().find(|(v, _)| v == vid).unwrap();
        let out = engine
            .make_prevote(&state, &staking, sk, vid)
            .unwrap()
            .unwrap();
        match out {
            Outbound::Vote(v) => {
                assert_eq!(v.phase, VotePhase::Prevote);
                assert_eq!(v.block_hash, Some(block_hash));
                prevotes.push(v);
            }
            _ => unreachable!(),
        }
    }

    for (_, engine) in engines.iter_mut() {
        for v in &prevotes {
            let _ = engine.on_vote(&state, &staking, v.clone()).unwrap();
        }
        assert_eq!(engine.lock_state().block_hash, Some(block_hash));
        assert_eq!(engine.step, Step::Precommit);
    }

    let mut precommits = Vec::new();
    for (vid, engine) in engines.iter_mut() {
        let (_, sk) = keys.iter().find(|(v, _)| v == vid).unwrap();
        let out = engine
            .make_precommit(&state, &staking, sk, vid)
            .unwrap()
            .unwrap();
        match out {
            Outbound::Vote(v) => {
                assert_eq!(v.phase, VotePhase::Precommit);
                assert_eq!(v.block_hash, Some(block_hash));
                precommits.push(v);
            }
            _ => unreachable!(),
        }
    }

    let mut committed: Option<axiom_primitives::Block> = None;
    for (_, engine) in engines.iter_mut() {
        for v in &precommits {
            let outs = engine.on_vote(&state, &staking, v.clone()).unwrap();
            for o in outs {
                if let Outbound::CommittedBlock(b) = o {
                    committed = Some(b);
                }
            }
        }
    }

    let committed = committed.expect("should commit");
    assert_eq!(committed.height, height);
    assert_eq!(committed.round, 0);
    assert_eq!(committed.protocol_version, ProtocolVersion::V2.as_u64());
    assert_eq!(compute_block_hash(&committed), block_hash);
    assert_eq!(committed.signatures.len(), 3);

    for sig in &committed.signatures {
        verify_precommit(
            &sig.validator_id,
            &block_hash,
            height,
            committed.round,
            &sig.signature,
        )
        .unwrap();
    }
}

#[test]
fn test_v2_lock_forces_prevote_in_later_round() {
    let (state, staking, keys) = mk_state_and_staking();
    let height = V2_ACTIVATION_HEIGHT + 1;
    let (vid, sk) = &keys[0];

    let mut engine = Engine::new(
        height,
        LockState {
            height,
            round: 0,
            block_hash: None,
        },
    );

    let proposer = engine.proposer_for_round(&state, &staking).unwrap();
    let (_, proposer_key) = keys.iter().find(|(v, _)| *v == proposer).unwrap();

    let out = engine
        .make_proposal(ProposalInputs {
            state: &state,
            staking: &staking,
            parent_hash: BlockHash([0u8; 32]),
            transactions: vec![],
            proposer_key,
            proposer_id: &proposer,
            timestamp: 0,
        })
        .unwrap();

    let proposal = match out {
        Outbound::Proposal(p) => p,
        _ => unreachable!(),
    };

    let h = compute_block_hash(&proposal.block);
    let _ = engine.on_proposal(&state, &staking, proposal).unwrap();
    let prevote = match engine
        .make_prevote(&state, &staking, sk, vid)
        .unwrap()
        .unwrap()
    {
        Outbound::Vote(v) => v,
        _ => unreachable!(),
    };
    let _ = engine.on_vote(&state, &staking, prevote).unwrap();

    engine.set_lock_state(LockState {
        height,
        round: 0,
        block_hash: Some(h),
    });
    engine.round = 1;
    engine.step = Step::Prevote;

    let forced = match engine
        .make_prevote(&state, &staking, sk, vid)
        .unwrap()
        .unwrap()
    {
        Outbound::Vote(v) => v,
        _ => unreachable!(),
    };

    assert_eq!(forced.phase, VotePhase::Prevote);
    assert_eq!(forced.block_hash, Some(h));
}
