#![deny(warnings)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use axiom_crypto::sign_transaction_for_height;
use axiom_primitives::{AccountId, Signature, Transaction, TransactionType};
use clap::Parser;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

const HEIGHT_REFRESH_EVERY: usize = 50;

#[derive(Parser)]
#[command(name = "tx-generator")]
struct Args {
    /// Node API URLs — each transaction is broadcast to all of them
    #[arg(
        long = "api-url",
        num_args = 1..,
        default_values = [
            "http://127.0.0.1:8081",
            "http://127.0.0.1:8082",
            "http://127.0.0.1:8083",
            "http://127.0.0.1:8084",
        ]
    )]
    api_urls: Vec<String>,

    /// Target transactions per second
    #[arg(long, default_value_t = 2.0)]
    tps: f64,

    /// Console username for auth
    #[arg(long, default_value = "operator")]
    username: String,

    /// Console password for auth
    #[arg(long, default_value = "axiom")]
    password: String,

    /// Directory containing test_account_N.secret files
    #[arg(long, default_value = "genesis_output")]
    keys_dir: PathBuf,
}

struct Account {
    signing_key: SigningKey,
    account_id: AccountId,
}

struct NodeState {
    url: String,
    token: String,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Deserialize)]
struct AccountInfo {
    nonce: u64,
}

#[derive(Deserialize)]
struct StatusInfo {
    height: u64,
}

#[derive(Deserialize)]
struct TxAccepted {
    tx_hash: String,
}

enum SubmitOutcome {
    Accepted(String),
    Unauthorized,
    Rejected(String),
}

fn load_accounts(keys_dir: &Path) -> Result<Vec<Account>, Box<dyn std::error::Error>> {
    let mut accounts = Vec::new();
    let mut n = 1u32;
    loop {
        let path = keys_dir.join(format!("test_account_{n}.secret"));
        if !path.exists() {
            break;
        }
        let hex_str = std::fs::read_to_string(&path)?;
        let bytes = hex::decode(hex_str.trim())?;
        if bytes.len() != 32 {
            return Err(format!(
                "{}: expected 32-byte hex, got {} bytes",
                path.display(),
                bytes.len()
            )
            .into());
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let signing_key = SigningKey::from_bytes(&arr);
        let account_id = AccountId(signing_key.verifying_key().to_bytes());
        info!(n, account_id = %account_id, "loaded account");
        accounts.push(Account { signing_key, account_id });
        n += 1;
    }
    if accounts.is_empty() {
        return Err(format!(
            "no test_account_N.secret files found in {}",
            keys_dir.display()
        )
        .into());
    }
    Ok(accounts)
}

async fn do_login(
    client: &reqwest::Client,
    api_url: &str,
    username: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let resp = client
        .post(format!("{api_url}/auth/login"))
        .json(&LoginRequest { username, password })
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("login failed ({status}): {body}").into());
    }
    let body: LoginResponse = resp.json().await?;
    Ok(body.token)
}

async fn fetch_nonce(
    client: &reqwest::Client,
    api_url: &str,
    account_id: &AccountId,
    token: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let resp = client
        .get(format!("{api_url}/api/accounts/{account_id}"))
        .bearer_auth(token)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(format!("fetch nonce ({account_id}) failed: {}", resp.status()).into());
    }
    let info: AccountInfo = resp.json().await?;
    Ok(info.nonce)
}

async fn fetch_height(
    client: &reqwest::Client,
    api_url: &str,
    token: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let resp = client
        .get(format!("{api_url}/api/status"))
        .bearer_auth(token)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(format!("fetch status failed: {}", resp.status()).into());
    }
    let info: StatusInfo = resp.json().await?;
    Ok(info.height)
}

async fn submit(
    client: &reqwest::Client,
    api_url: &str,
    token: &str,
    tx: &Transaction,
) -> Result<SubmitOutcome, Box<dyn std::error::Error>> {
    let resp = client
        .post(format!("{api_url}/api/transactions"))
        .bearer_auth(token)
        .json(tx)
        .send()
        .await?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Ok(SubmitOutcome::Unauthorized);
    }
    if status == reqwest::StatusCode::ACCEPTED {
        let body: TxAccepted = resp.json().await?;
        return Ok(SubmitOutcome::Accepted(body.tx_hash));
    }
    let body = resp.text().await.unwrap_or_default();
    Ok(SubmitOutcome::Rejected(format!("{status}: {body}")))
}

fn build_tx(sender: &Account, recipient_id: AccountId, nonce: u64, next_height: u64) -> Transaction {
    let tx = Transaction {
        sender: sender.account_id,
        recipient: recipient_id,
        amount: 1,
        nonce,
        signature: Signature([0u8; 64]),
        tx_type: TransactionType::Transfer,
        evidence: None,
    };
    let sig = sign_transaction_for_height(next_height, &sender.signing_key, &tx);
    Transaction { signature: sig, ..tx }
}

/// Submit to one node; re-login on 401. Returns the tx hash if accepted.
async fn submit_to_node(
    client: &reqwest::Client,
    node: &mut NodeState,
    username: &str,
    password: &str,
    tx: &Transaction,
) -> Option<String> {
    let outcome = match submit(client, &node.url, &node.token, tx).await {
        Ok(o) => o,
        Err(e) => {
            warn!(url = %node.url, err = %e, "network error submitting tx");
            return None;
        }
    };
    match outcome {
        SubmitOutcome::Accepted(hash) => Some(hash),
        SubmitOutcome::Unauthorized => {
            warn!(url = %node.url, "401, re-logging in");
            match do_login(client, &node.url, username, password).await {
                Ok(t) => {
                    node.token = t;
                    match submit(client, &node.url, &node.token, tx).await {
                        Ok(SubmitOutcome::Accepted(hash)) => Some(hash),
                        Ok(SubmitOutcome::Unauthorized) => {
                            error!(url = %node.url, "still 401 after re-login");
                            None
                        }
                        Ok(SubmitOutcome::Rejected(msg)) => {
                            error!(url = %node.url, err = %msg, "tx rejected after re-login");
                            None
                        }
                        Err(e) => {
                            error!(url = %node.url, err = %e, "network error after re-login");
                            None
                        }
                    }
                }
                Err(e) => {
                    error!(url = %node.url, err = %e, "re-login failed");
                    None
                }
            }
        }
        SubmitOutcome::Rejected(msg) => {
            error!(url = %node.url, err = %msg, "tx rejected");
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if args.tps <= 0.0 {
        return Err("--tps must be positive".into());
    }

    let accounts = load_accounts(&args.keys_dir)?;
    let n = accounts.len();
    info!(n, "accounts loaded");

    let client = reqwest::Client::new();

    let mut nodes: Vec<NodeState> = Vec::new();
    for url in &args.api_urls {
        match do_login(&client, url, &args.username, &args.password).await {
            Ok(token) => {
                info!(url, "logged in");
                nodes.push(NodeState { url: url.clone(), token });
            }
            Err(e) => {
                warn!(url, err = %e, "login failed, skipping node");
            }
        }
    }
    if nodes.is_empty() {
        return Err("failed to login to any node".into());
    }

    let mut nonces: Vec<u64> = Vec::with_capacity(n);
    for acc in &accounts {
        let nonce =
            fetch_nonce(&client, &nodes[0].url, &acc.account_id, &nodes[0].token).await?;
        info!(account_id = %acc.account_id, nonce, "nonce fetched");
        nonces.push(nonce);
    }

    let mut height = fetch_height(&client, &nodes[0].url, &nodes[0].token).await?;
    info!(height, "initial chain height");

    let interval = Duration::from_secs_f64(1.0 / args.tps);

    info!(
        tps = args.tps,
        interval_ms = interval.as_millis(),
        nodes = nodes.len(),
        "generator ready"
    );

    let mut i: usize = 0;
    loop {
        if i > 0 && i % HEIGHT_REFRESH_EVERY == 0 {
            let url = nodes[0].url.clone();
            let token = nodes[0].token.clone();
            match fetch_height(&client, &url, &token).await {
                Ok(h) => {
                    height = h;
                    info!(height, "refreshed chain height");
                }
                Err(e) => warn!(err = %e, "failed to refresh chain height"),
            }
        }

        let sender_idx = i % n;
        let recipient_idx = (i + 1) % n;
        let next_height = height.saturating_add(1);
        let nonce = nonces[sender_idx];
        let recipient_id = accounts[recipient_idx].account_id;

        let tx = build_tx(&accounts[sender_idx], recipient_id, nonce, next_height);

        let mut first_hash: Option<String> = None;
        for node in &mut nodes {
            if let Some(hash) =
                submit_to_node(&client, node, &args.username, &args.password, &tx).await
            {
                if first_hash.is_none() {
                    first_hash = Some(hash);
                }
            }
        }

        if let Some(hash) = first_hash {
            info!(tx_hash = %hash, nonce, sender_idx, "tx accepted");
            nonces[sender_idx] = nonce.saturating_add(1);
        } else {
            warn!(nonce, sender_idx, "tx not accepted by any node");
        }

        i += 1;
        tokio::time::sleep(interval).await;
    }
}
