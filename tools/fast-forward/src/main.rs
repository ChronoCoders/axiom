use axiom_crypto::compute_block_hash;
use axiom_execution::{compute_state_hash, execute_proposal, select_proposer};
use axiom_primitives::{Block, BlockHash, ProtocolVersion, StateHash, ValidatorId, PROTOCOL_VERSION_V1, V2_ACTIVATION_HEIGHT};
use axiom_state::State;
use axiom_storage::Storage;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("Usage: fast-forward <sqlite_path> [target_height] [genesis_json_path]");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
    }

    let sqlite_path = PathBuf::from(args.remove(0));
    let target_height = if !args.is_empty() {
        args[0].parse::<u64>().unwrap_or_else(|_| usage())
    } else {
        V2_ACTIVATION_HEIGHT.saturating_sub(1)
    };
    if !args.is_empty() {
        args.remove(0);
    }

    let genesis_path = if !args.is_empty() {
        PathBuf::from(args.remove(0))
    } else {
        PathBuf::from("docs/reference_genesis.json")
    };

    let genesis_json = std::fs::read_to_string(&genesis_path)
        .unwrap_or_else(|e| panic!("Failed to read genesis: {genesis_path:?}: {e}"));
    let genesis = axiom_primitives::deserialize_genesis_json(&genesis_json)
        .unwrap_or_else(|e| panic!("Failed to parse genesis: {e}"));
    let mut state = State::from_genesis(&genesis).unwrap();

    let storage = Storage::initialize(
        sqlite_path
            .to_str()
            .unwrap_or_else(|| panic!("Invalid sqlite_path: {sqlite_path:?}")),
    )
    .unwrap();

    let genesis_hash = compute_state_hash(&state);
    storage.store_genesis(&state, &genesis_hash).unwrap();

    let genesis_block = Block {
        parent_hash: BlockHash([0u8; 32]),
        height: 0,
        epoch: 0,
        protocol_version: PROTOCOL_VERSION_V1,
        round: 0,
        proposer_id: ValidatorId([0u8; 32]),
        transactions: vec![],
        signatures: vec![],
        state_hash: genesis_hash,
        timestamp: 0,
    };
    storage.commit_block(&genesis_block, &state).unwrap();

    let mut parent_hash = compute_block_hash(&genesis_block);

    for h in 1..=target_height {
        let proposer = select_proposer(&state, h).unwrap();
        let (next_state, state_hash) = execute_proposal(&state, &[], &proposer).unwrap();

        let block = Block {
            parent_hash,
            height: h,
            epoch: 0,
            protocol_version: ProtocolVersion::V1.as_u64(),
            round: 0,
            proposer_id: proposer,
            transactions: vec![],
            signatures: vec![],
            state_hash,
            timestamp: 0,
        };

        storage.commit_block(&block, &next_state).unwrap();
        parent_hash = compute_block_hash(&block);
        state = next_state;
    }

    let final_hash: StateHash = compute_state_hash(&state);
    println!(
        "ok height={} sqlite_path={} state_hash={}",
        target_height,
        sqlite_path.display(),
        final_hash
    );
}

