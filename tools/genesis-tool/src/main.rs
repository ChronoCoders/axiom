#![deny(warnings)]

use axiom_crypto::compute_genesis_hash;
use axiom_primitives::{AccountId, GenesisAccount, GenesisConfig, GenesisValidator, ValidatorId};
use chrono::Utc;
use clap::Parser;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "4")]
    validators: usize,
    #[arg(long, default_value = "1000000")]
    initial_balance: u64,
    #[arg(long, default_value = "10")]
    voting_power: u64,
    #[arg(long, default_value = "10")]
    block_reward: u64,
    #[arg(long, default_value = "4")]
    test_accounts: usize,
    #[arg(long, default_value = "1000000")]
    test_balance: u64,
    #[arg(long, default_value = "genesis_output")]
    output_dir: PathBuf,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.output_dir.exists() {
        let non_empty = fs::read_dir(&args.output_dir)?.next().is_some();
        if non_empty {
            eprintln!(
                "error: '{}' already exists and is not empty",
                args.output_dir.display()
            );
            std::process::exit(1);
        }
    } else {
        fs::create_dir_all(&args.output_dir)?;
    }

    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut accounts: Vec<GenesisAccount> = Vec::new();
    let mut validators: Vec<GenesisValidator> = Vec::new();
    let mut val_records: Vec<(ValidatorId, AccountId)> = Vec::new();
    let mut test_records: Vec<AccountId> = Vec::new();

    for i in 1..=args.validators {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing_key.verifying_key().to_bytes();
        let priv_bytes = signing_key.to_bytes();

        write_secret(
            &args.output_dir.join(format!("validator_{i}.secret")),
            &hex::encode(priv_bytes),
        )?;

        let account_id = AccountId(pub_bytes);
        let validator_id = ValidatorId(pub_bytes);

        accounts.push(GenesisAccount {
            id: account_id,
            balance: args.initial_balance,
            nonce: 0,
        });
        validators.push(GenesisValidator {
            id: validator_id,
            voting_power: args.voting_power,
            account_id,
            active: true,
        });
        val_records.push((validator_id, account_id));
    }

    for i in 1..=args.test_accounts {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing_key.verifying_key().to_bytes();
        let priv_bytes = signing_key.to_bytes();

        write_secret(
            &args.output_dir.join(format!("test_account_{i}.secret")),
            &hex::encode(priv_bytes),
        )?;

        let account_id = AccountId(pub_bytes);
        accounts.push(GenesisAccount {
            id: account_id,
            balance: args.test_balance,
            nonce: 0,
        });
        test_records.push(account_id);
    }

    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    validators.sort_by(|a, b| a.id.cmp(&b.id));

    let total_supply = (args.validators as u64) * args.initial_balance
        + (args.test_accounts as u64) * args.test_balance;

    let genesis = GenesisConfig {
        accounts,
        block_reward: args.block_reward,
        total_supply,
        validators,
    };

    let json = serde_json::to_string(&genesis)?;
    fs::write(args.output_dir.join("genesis.json"), json.as_bytes())?;

    let hash = compute_genesis_hash(&genesis);
    let hash_hex = hex::encode(hash.0);

    fs::write(args.output_dir.join("genesis_hash.txt"), hash_hex.as_bytes())?;

    let mut t = String::new();
    writeln!(t, "AXIOM Genesis Ceremony")?;
    writeln!(t, "======================")?;
    writeln!(t, "Timestamp: {timestamp}")?;
    writeln!(t, "Validators: {}", args.validators)?;
    writeln!(t, "Test Accounts: {}", args.test_accounts)?;
    writeln!(t, "Block Reward: {}", args.block_reward)?;
    writeln!(t, "Total Supply: {total_supply}")?;
    writeln!(t)?;
    writeln!(t, "Validators")?;
    writeln!(t, "----------")?;
    for (i, (vid, aid)) in val_records.iter().enumerate() {
        writeln!(
            t,
            "{}  validator_id: {vid}  account_id: {aid}  voting_power: {}  balance: {}",
            i + 1,
            args.voting_power,
            args.initial_balance,
        )?;
    }
    writeln!(t)?;
    writeln!(t, "Test Accounts")?;
    writeln!(t, "-------------")?;
    for (i, aid) in test_records.iter().enumerate() {
        writeln!(
            t,
            "{}  account_id: {aid}  balance: {}",
            i + 1,
            args.test_balance,
        )?;
    }
    writeln!(t)?;
    writeln!(t, "Genesis Hash")?;
    writeln!(t, "------------")?;
    writeln!(t, "{hash_hex}")?;
    writeln!(t)?;
    writeln!(
        t,
        "LOCKED_GENESIS_HASH constant (paste into node/src/main.rs):"
    )?;
    writeln!(
        t,
        "------------------------------------------------------------"
    )?;
    writeln!(t, "const LOCKED_GENESIS_HASH: &str = \"{hash_hex}\";")?;

    fs::write(args.output_dir.join("ceremony_transcript.txt"), t.as_bytes())?;

    println!("Genesis ceremony complete.");
    println!("Output: {}", args.output_dir.display());
    println!("Genesis hash: {hash_hex}");

    Ok(())
}

fn write_secret(path: &std::path::Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, content.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
