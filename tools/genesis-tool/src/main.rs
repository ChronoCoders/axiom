use axiom_crypto::compute_genesis_hash;
use axiom_primitives::{AccountId, GenesisAccount, GenesisConfig, GenesisValidator, ValidatorId};
use clap::Parser;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "genesis.json")]
    output: PathBuf,
    #[arg(long, default_value = "4")]
    validators: usize,
}

fn main() {
    let args = Args::parse();
    let mut accounts = Vec::new();
    let mut validators = Vec::new();

    println!("Generating {} validators...", args.validators);

    for i in 1..=args.validators {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_key = signing_key.verifying_key();
        let pub_bytes = pub_key.to_bytes();
        let priv_bytes = signing_key.to_bytes();

        // Save private key
        let key_filename = format!("validator_{i}.secret");
        let hex_key = hex::encode(priv_bytes);
        let mut file = match File::create(&key_filename) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create {key_filename}: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = file.write_all(hex_key.as_bytes()) {
            eprintln!("Failed to write {key_filename}: {e}");
            std::process::exit(1);
        }
        println!("Saved {key_filename}");

        let account_id = AccountId(pub_bytes);
        let validator_id = ValidatorId(pub_bytes);

        accounts.push(GenesisAccount {
            id: account_id,
            balance: 1_000_000, // 1M coins
            nonce: 0,
        });

        validators.push(GenesisValidator {
            id: validator_id,
            voting_power: 10,
            account_id,
            active: true,
        });
    }

    // Sort to ensure deterministic output
    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    validators.sort_by(|a, b| a.id.cmp(&b.id));

    let genesis = GenesisConfig {
        total_supply: 1_000_000 * args.validators as u64,
        block_reward: 10,
        accounts,
        validators,
    };

    let json = match serde_json::to_string(&genesis) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize genesis: {e}");
            std::process::exit(1);
        }
    };

    let mut file = match File::create(&args.output) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create {:?}: {e}", args.output);
            std::process::exit(1);
        }
    };
    if let Err(e) = file.write_all(json.as_bytes()) {
        eprintln!("Failed to write {:?}: {e}", args.output);
        std::process::exit(1);
    }
    println!("Saved genesis to {:?}", args.output);

    let hash = compute_genesis_hash(&genesis);
    println!("Genesis Hash: {}", hex::encode(hash.0));
}
