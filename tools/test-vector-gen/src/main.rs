use axiom_crypto::{
    compute_block_hash, compute_genesis_hash, generate_keypair_from_seed, sha256, sign_vote,
};
use axiom_execution::{compute_state_hash, select_proposer};
use axiom_primitives::{
    to_hex, AccountId, Block, BlockHash, GenesisAccount, GenesisConfig, GenesisValidator,
    TransactionType, ValidatorId, ValidatorSignature,
};
use axiom_state::State;

const SEEDS: [&str; 4] = [
    "axiom-test-validator-1",
    "axiom-test-validator-2",
    "axiom-test-validator-3",
    "axiom-test-validator-4",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== AXIOM Protocol v1 — Deterministic Test Vector Generation ===\n");

    let mut keys = Vec::new();
    let mut accounts = Vec::new();
    let mut validators = Vec::new();

    for (i, seed_str) in SEEDS.iter().enumerate() {
        let seed = sha256(seed_str.as_bytes());
        let (sk, pk) = generate_keypair_from_seed(&seed);
        let account_id = AccountId(pk.0);
        let validator_id = ValidatorId(pk.0);

        let label = (b'A' + i as u8) as char;
        let num = i + 1;
        let seed_hex = to_hex(&seed);
        let priv_hex = to_hex(&sk.to_bytes());
        let pub_hex = to_hex(&pk.0);
        println!("Validator-{num} / Account-{label}:");
        println!("  Seed String: \"{seed_str}\"");
        println!("  Seed (SHA-256): {seed_hex}");
        println!("  Private Key: {priv_hex}");
        println!("  Public Key:  {pub_hex}");
        println!();

        accounts.push(GenesisAccount {
            id: account_id,
            balance: 1_000_000,
            nonce: 0,
        });

        validators.push(GenesisValidator {
            id: validator_id,
            voting_power: 10,
            account_id,
            active: true,
        });

        keys.push((sk, pk, account_id, validator_id));
    }

    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    validators.sort_by(|a, b| a.id.cmp(&b.id));

    let genesis = GenesisConfig {
        total_supply: 4_000_000,
        block_reward: 10,
        accounts,
        validators,
    };

    let genesis_state_hash = compute_genesis_hash(&genesis);
    let hash_hex = to_hex(&genesis_state_hash.0);
    println!("--- Genesis ---");
    println!("Genesis State Hash: {hash_hex}");
    println!();

    let genesis_json = serde_json::to_string(&genesis)?;
    println!("Genesis JSON (canonical):");
    println!("{genesis_json}");
    println!();

    let state = State::from_genesis(&genesis)?;

    println!("--- Sorted Validator Order (BTreeMap) ---");
    for (i, (vid, val)) in state.active_validators().iter().enumerate() {
        let label_idx = keys
            .iter()
            .position(|(_, _, _, v)| v == *vid)
            .ok_or("Failed to map validator id to seed label")?;
        let prefix = &to_hex(&vid.0)[..16];
        let num = label_idx + 1;
        let vp = val.voting_power;
        println!("  Index {i}: Validator-{num} ({prefix}...)");
        println!("    Voting Power: {vp}");
    }
    println!();

    println!("--- Proposer Selection ---");
    for h in 1u64..=8 {
        let proposer = select_proposer(&state, h)?;
        let label_idx = keys
            .iter()
            .position(|(_, _, _, v)| *v == proposer)
            .ok_or("Failed to map proposer id to seed label")?;
        let idx = h as usize % 4;
        let num = label_idx + 1;
        let prefix = &to_hex(&proposer.0)[..16];
        println!("  Height {h}: index {h} % 4 = {idx} → Validator-{num} ({prefix}...)");
    }
    println!();

    println!("--- Quorum Parameters ---");
    let total_power = state.total_voting_power()?;
    let double_total = total_power * 2;
    println!("  Total Voting Power: {total_power}");
    println!("  Quorum Rule: 3 * collected > 2 * total");
    println!("  3 * collected > 2 * {total_power} = {double_total}");
    println!("  Minimum collected: 30 (3 validators × 10 power)");
    println!("  Signatures required: 3 of 4");
    println!();

    println!("=== Block Vector 1: Empty Block at Height 1 ===\n");

    let proposer_h1 = select_proposer(&state, 1)?;
    let proposer_h1_idx = keys
        .iter()
        .position(|(_, _, _, v)| *v == proposer_h1)
        .ok_or("Failed to map proposer id at height 1")?;
    let p1_num = proposer_h1_idx + 1;
    println!("Proposer at height 1: Validator-{p1_num}");

    let parent_hash = BlockHash([0u8; 32]);
    let parent_hex = to_hex(&parent_hash.0);
    println!("Parent hash (genesis block hash placeholder): {parent_hex}");

    let (state_after_b1, state_hash_h1) =
        axiom_execution::execute_proposal(&state, &[], &proposer_h1)?;
    let sh1_hex = to_hex(&state_hash_h1.0);
    println!("State hash after block 1: {sh1_hex}");

    let mut block1 = Block {
        parent_hash,
        height: 1,
        epoch: 0,
        protocol_version: axiom_primitives::PROTOCOL_VERSION_V1,
        round: 0,
        proposer_id: proposer_h1,
        transactions: vec![],
        signatures: vec![],
        state_hash: state_hash_h1,
        timestamp: 0,
    };

    let block1_hash_unsigned = compute_block_hash(&block1);
    let bh1_hex = to_hex(&block1_hash_unsigned.0);
    println!("Block 1 hash (unsigned): {bh1_hex}");

    let mut sorted_keys: Vec<(usize, &_)> = keys.iter().enumerate().collect();
    sorted_keys.sort_by(|a, b| a.1 .3.cmp(&b.1 .3));

    for (orig_idx, (sk, _, _, vid)) in &sorted_keys {
        let sig = sign_vote(sk, &block1_hash_unsigned, 1);
        block1.signatures.push(ValidatorSignature {
            validator_id: *vid,
            signature: sig,
        });
        let num = orig_idx + 1;
        let sig_prefix = &to_hex(&sig.0)[..32];
        println!("  Validator-{num} signature: {sig_prefix}...");
    }

    let block1_hash_signed = compute_block_hash(&block1);
    let bh1s_hex = to_hex(&block1_hash_signed.0);
    println!("\nBlock 1 hash (note: signatures not in block hash): {bh1s_hex}");
    println!("(Block hash should equal unsigned hash since signatures are excluded from canonical serialization)");

    println!("\n=== Block Vector 2: Empty Block at Height 2 ===\n");

    let proposer_h2 = select_proposer(&state_after_b1, 2)?;
    let proposer_h2_idx = keys
        .iter()
        .position(|(_, _, _, v)| *v == proposer_h2)
        .ok_or("Failed to map proposer id at height 2")?;
    let p2_num = proposer_h2_idx + 1;
    println!("Proposer at height 2: Validator-{p2_num}");

    let (_, state_hash_h2_empty) =
        axiom_execution::execute_proposal(&state_after_b1, &[], &proposer_h2)?;
    let sh2e_hex = to_hex(&state_hash_h2_empty.0);
    println!("State hash after block 2 (empty): {sh2e_hex}");

    let block2_empty = Block {
        parent_hash: block1_hash_unsigned,
        height: 2,
        epoch: 0,
        protocol_version: axiom_primitives::PROTOCOL_VERSION_V1,
        round: 0,
        proposer_id: proposer_h2,
        transactions: vec![],
        signatures: vec![],
        state_hash: state_hash_h2_empty,
        timestamp: 0,
    };

    let block2_empty_hash = compute_block_hash(&block2_empty);
    let bh2e_hex = to_hex(&block2_empty_hash.0);
    println!("Block 2 hash (empty): {bh2e_hex}");

    println!("\n=== Transaction Vector 1: Transfer 100,000 from account-D to account-A (Block 2) ===\n");

    let sender_d_idx = 0;
    let recipient_a_idx = 1;
    let (ref sender_d_sk, _, ref sender_d_account, _) = keys[sender_d_idx];
    let (_, _, ref recipient_a_account, _) = keys[recipient_a_idx];

    println!("Sender: account-D (Validator-1, sorted index 3)");
    println!("  Account ID: {}", to_hex(&sender_d_account.0));
    println!("Recipient: account-A (Validator-2, sorted index 0)");
    println!("  Account ID: {}", to_hex(&recipient_a_account.0));
    println!("Amount: 100,000 AXM");
    println!("Nonce: 0");

    let tx1_unsigned = axiom_primitives::Transaction {
        sender: *sender_d_account,
        recipient: *recipient_a_account,
        amount: 100_000,
        nonce: 0,
        signature: axiom_primitives::Signature([0u8; 64]),
        tx_type: TransactionType::Transfer,
    };

    let tx1_hash = axiom_crypto::compute_transaction_hash(&tx1_unsigned);
    let tx1h_hex = to_hex(&tx1_hash.0);
    println!("Transaction hash: {tx1h_hex}");

    let tx1_sig = axiom_crypto::sign_transaction(sender_d_sk, &tx1_unsigned);
    let tx1_sig_hex = to_hex(&tx1_sig.0);
    println!("Transaction signature: {tx1_sig_hex}");

    let tx1_signed = axiom_primitives::Transaction {
        sender: *sender_d_account,
        recipient: *recipient_a_account,
        amount: 100_000,
        nonce: 0,
        signature: tx1_sig,
        tx_type: TransactionType::Transfer,
    };

    let (state_after_b2, state_hash_h2) = axiom_execution::execute_proposal(
        &state_after_b1,
        std::slice::from_ref(&tx1_signed),
        &proposer_h2,
    )
    ?;
    let sh2_hex = to_hex(&state_hash_h2.0);
    println!("\nState hash after block 2 (with transfer): {sh2_hex}");
    println!("  STATE_HASH_BLOCK_2 = {sh2_hex}");

    println!("\n  Post-block 2 balances:");
    for (i, (_, _, aid, _)) in keys.iter().enumerate() {
        if let Some(acc) = state_after_b2.accounts.get(aid) {
            let label_sorted = sorted_account_label(&state_after_b2, aid);
            let vnum = i + 1;
            println!(
                "    Validator-{vnum} / {label_sorted}: balance={}, nonce={}",
                acc.balance, acc.nonce
            );
        }
    }
    println!("  total_supply: {}", state_after_b2.total_supply);

    println!("\n=== Transaction Vector 4: Transfer 50,000 from account-D to account-E (Block 3) ===\n");

    let account_e_seed = sha256(b"axiom-test-account-5");
    let (_, account_e_pk) = generate_keypair_from_seed(&account_e_seed);
    let account_e_id = AccountId(account_e_pk.0);
    let ae_hex = to_hex(&account_e_id.0);
    println!("account-E (auto-created):");
    println!("  Seed String: \"axiom-test-account-5\"");
    println!("  Seed (SHA-256): {}", to_hex(&account_e_seed));
    println!("  Public Key / Account ID: {ae_hex}");

    let proposer_h3 = select_proposer(&state_after_b2, 3)?;
    let proposer_h3_idx = keys
        .iter()
        .position(|(_, _, _, v)| *v == proposer_h3)
        .ok_or("Failed to map proposer id at height 3")?;
    let p3_num = proposer_h3_idx + 1;
    println!("\nProposer at height 3: Validator-{p3_num}");

    println!("Sender: account-D (Validator-1)");
    println!("Recipient: account-E (does not exist, auto-create)");
    println!("Amount: 50,000 AXM");
    println!("Nonce: 1");

    let tx2_unsigned = axiom_primitives::Transaction {
        sender: *sender_d_account,
        recipient: account_e_id,
        amount: 50_000,
        nonce: 1,
        signature: axiom_primitives::Signature([0u8; 64]),
        tx_type: TransactionType::Transfer,
    };

    let tx2_hash = axiom_crypto::compute_transaction_hash(&tx2_unsigned);
    let tx2h_hex = to_hex(&tx2_hash.0);
    println!("Transaction hash: {tx2h_hex}");

    let tx2_sig = axiom_crypto::sign_transaction(sender_d_sk, &tx2_unsigned);
    let tx2_sig_hex = to_hex(&tx2_sig.0);
    println!("Transaction signature: {tx2_sig_hex}");

    let tx2_signed = axiom_primitives::Transaction {
        sender: *sender_d_account,
        recipient: account_e_id,
        amount: 50_000,
        nonce: 1,
        signature: tx2_sig,
        tx_type: TransactionType::Transfer,
    };

    let (state_after_b3, state_hash_h3) = axiom_execution::execute_proposal(
        &state_after_b2,
        std::slice::from_ref(&tx2_signed),
        &proposer_h3,
    )
    ?;
    let sh3_hex = to_hex(&state_hash_h3.0);
    println!("\nState hash after block 3 (with auto-create transfer): {sh3_hex}");
    println!("  STATE_HASH_BLOCK_3 = {sh3_hex}");

    println!("\n  Post-block 3 balances:");
    for (i, (_, _, aid, _)) in keys.iter().enumerate() {
        if let Some(acc) = state_after_b3.accounts.get(aid) {
            let label_sorted = sorted_account_label(&state_after_b3, aid);
            let vnum = i + 1;
            println!(
                "    Validator-{vnum} / {label_sorted}: balance={}, nonce={}",
                acc.balance, acc.nonce
            );
        }
    }
    if let Some(acc_e) = state_after_b3.accounts.get(&account_e_id) {
        println!(
            "    account-E: balance={}, nonce={}",
            acc_e.balance, acc_e.nonce
        );
    }
    println!("  total_supply: {}", state_after_b3.total_supply);

    let state_hash_verify = compute_state_hash(&state_after_b3);
    let verify_hex = to_hex(&state_hash_verify.0);
    println!("\n  State hash verification (recomputed): {verify_hex}");
    if state_hash_h3 != state_hash_verify {
        return Err("State hash mismatch after block 3".into());
    }

    println!("\n=== Transfer Vector (original) ===\n");

    let sender_idx = 0;
    let recipient_idx = 1;
    let (ref sender_sk, _, ref sender_account, _) = keys[sender_idx];
    let (_, _, ref recipient_account, _) = keys[recipient_idx];

    let s_num = sender_idx + 1;
    let s_label = (b'A' + sender_idx as u8) as char;
    let r_num = recipient_idx + 1;
    let r_label = (b'A' + recipient_idx as u8) as char;
    println!("Sender: Validator-{s_num} / Account-{s_label}");
    println!("Recipient: Validator-{r_num} / Account-{r_label}");
    println!("Amount: 50,000 AXM");
    println!("Nonce: 1");

    let tx = axiom_primitives::Transaction {
        sender: *sender_account,
        recipient: *recipient_account,
        amount: 50_000,
        nonce: 1,
        signature: axiom_primitives::Signature([0u8; 64]),
        tx_type: TransactionType::Transfer,
    };

    let tx_hash = axiom_crypto::compute_transaction_hash(&tx);
    let txh_hex = to_hex(&tx_hash.0);
    println!("Transaction hash: {txh_hex}");

    let signed_tx = axiom_primitives::Transaction {
        sender: *sender_account,
        recipient: *recipient_account,
        amount: 50_000,
        nonce: 1,
        signature: axiom_crypto::sign_transaction(sender_sk, &tx),
        tx_type: TransactionType::Transfer,
    };
    let sig_prefix = &to_hex(&signed_tx.signature.0)[..32];
    println!("Transaction signature: {sig_prefix}...");

    println!("\n=== Reference genesis.json written to stdout above ===");
    println!("\n=== Generation Complete ===");
    Ok(())
}

fn sorted_account_label(state: &State, account_id: &AccountId) -> String {
    let mut all_ids: Vec<&AccountId> = state.accounts.keys().collect();
    all_ids.sort();
    let pos = all_ids.iter().position(|id| *id == account_id);
    match pos {
        Some(idx) => format!("account-{}", (b'A' + idx as u8) as char),
        None => format!("account-??({}...)", &to_hex(&account_id.0)[..8]),
    }
}
