use axiom_crypto::{compute_transaction_hash, verify_transaction_signature};
use axiom_mempool::Mempool;
use axiom_network::PeerMap;
use axiom_primitives::{
    serialize_transaction_canonical, AccountId, BlockHash, Transaction, ValidatorSignature,
    PROTOCOL_VERSION,
};
use axiom_storage::Storage;
use axum::{
    error_handling::HandleErrorLayer,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    BoxError, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::buffer::BufferLayer;
use tower::limit::RateLimitLayer;
use tower::load_shed::LoadShedLayer;
use tower::timeout::TimeoutLayer;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tokio::sync::RwLock;

/// Shared application state passed to all API handlers.
pub struct AppState {
    /// Persistent block and state storage.
    pub storage: Arc<Storage>,
    /// In-memory transaction pool.
    pub mempool: Arc<Mutex<Mempool>>,
    /// Live map of connected P2P peers.
    pub peers: PeerMap,
    /// Active auth tokens for the console UI (in-memory).
    pub auth_tokens: Arc<RwLock<HashSet<String>>>,
    /// Console login username.
    pub console_user: String,
    /// Console login password.
    pub console_pass: String,
    /// Monotonic counter for token generation uniqueness.
    pub token_counter: AtomicU64,
    /// Maximum size of a single transaction in canonical bytes.
    pub max_tx_bytes: usize,
}

async fn handle_error(error: BoxError) -> (StatusCode, String) {
    if error.is::<tower::timeout::error::Elapsed>() {
        (StatusCode::REQUEST_TIMEOUT, "Request timed out".to_string())
    } else if error.is::<tower::load_shed::error::Overloaded>() {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded".to_string(),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled internal error: {error}"),
        )
    }
}

// API Error Response
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
}

impl ApiError {
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
        }
    }
}

// Health Checks
async fn health_live() -> StatusCode {
    StatusCode::OK
}

async fn health_ready(State(state): State<Arc<AppState>>) -> StatusCode {
    // Check if storage is accessible and genesis is loaded
    match state.storage.get_genesis_hash() {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[derive(Deserialize)]
struct AuthLoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct AuthTokenRequest {
    token: String,
}

#[derive(Serialize)]
struct AuthLoginResponse {
    token: String,
}

#[derive(Serialize)]
struct AuthVerifyResponse {
    valid: bool,
}

#[derive(Serialize)]
struct AuthLogoutResponse {
    ok: bool,
}

async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthLoginRequest>,
) -> Result<Json<AuthLoginResponse>, (StatusCode, Json<ApiError>)> {
    if req.username != state.console_user || req.password != state.console_pass {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("Invalid credentials", "unauthorized")),
        ));
    }

    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = state.token_counter.fetch_add(1, Ordering::Relaxed);
    let token_material = format!("{}:{}:{}:{}", req.username, now_nanos, n, req.password);
    let token = hex::encode(axiom_crypto::sha256(token_material.as_bytes()));

    let mut tokens = state.auth_tokens.write().await;
    tokens.insert(token.clone());

    Ok(Json(AuthLoginResponse { token }))
}

async fn auth_verify(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthTokenRequest>,
) -> Result<Json<AuthVerifyResponse>, StatusCode> {
    let tokens = state.auth_tokens.read().await;
    if tokens.contains(&req.token) {
        Ok(Json(AuthVerifyResponse { valid: true }))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn auth_logout(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthTokenRequest>,
) -> Json<AuthLogoutResponse> {
    let mut tokens = state.auth_tokens.write().await;
    tokens.remove(&req.token);
    Json(AuthLogoutResponse { ok: true })
}

// Status
#[derive(Serialize)]
struct StatusResponse {
    protocol_version: u64,
    node_version: String,
    height: u64,
    latest_block_hash: String,
    genesis_hash: String,
    validator_count: usize,
    syncing: bool,
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ApiError>)> {
    let height = state.storage.get_latest_height().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;
    let genesis_hash = state.storage.get_genesis_hash().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    // Get latest block hash
    let latest_block = state.storage.get_block_by_height(height).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    let latest_block_hash = if let Some(block) = latest_block {
        axiom_crypto::compute_block_hash(&block).to_string()
    } else {
        String::new()
    };

    let validators = state.storage.get_validators().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    Ok(Json(StatusResponse {
        protocol_version: PROTOCOL_VERSION,
        node_version: env!("CARGO_PKG_VERSION").to_string(),
        height,
        latest_block_hash,
        genesis_hash: genesis_hash.to_string(),
        validator_count: validators.len(),
        syncing: false, // Always false for now
    }))
}

// Blocks
#[derive(Deserialize)]
struct ListBlocksParams {
    limit: Option<usize>,
    cursor: Option<u64>,
}

#[derive(Serialize)]
struct BlockSummary {
    height: u64,
    hash: String,
    parent_hash: String,
    epoch: u64,
    proposer_id: String,
    transaction_count: usize,
    state_hash: String,
    timestamp: String,
}

async fn list_blocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListBlocksParams>,
) -> Result<Json<Vec<BlockSummary>>, (StatusCode, Json<ApiError>)> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let latest_height = state.storage.get_latest_height().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    let start_height = params.cursor.unwrap_or(latest_height);
    let mut blocks = Vec::new();

    // Iterate backwards from start_height
    for h in (0..=start_height).rev().take(limit) {
        if let Some(block) = state.storage.get_block_by_height(h).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(e.to_string(), "storage_error")),
            )
        })? {
            let hash = axiom_crypto::compute_block_hash(&block);
            blocks.push(BlockSummary {
                height: block.height,
                hash: hash.to_string(),
                parent_hash: block.parent_hash.to_string(),
                epoch: block.epoch,
                proposer_id: block.proposer_id.to_string(),
                transaction_count: block.transactions.len(),
                state_hash: block.state_hash.to_string(),
                timestamp: format_unix_timestamp(block.timestamp),
            });
        }
    }

    Ok(Json(blocks))
}

#[derive(Serialize)]
struct BlockDetail {
    #[serde(flatten)]
    summary: BlockSummary,
    transactions: Vec<Transaction>,
    signatures: Vec<ValidatorSignature>,
}

async fn get_block_by_height(
    State(state): State<Arc<AppState>>,
    Path(height): Path<u64>,
) -> Result<Json<BlockDetail>, (StatusCode, Json<ApiError>)> {
    let block = state.storage.get_block_by_height(height).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    if let Some(block) = block {
        let hash = axiom_crypto::compute_block_hash(&block);
        Ok(Json(BlockDetail {
            summary: BlockSummary {
                height: block.height,
                hash: hash.to_string(),
                parent_hash: block.parent_hash.to_string(),
                epoch: block.epoch,
                proposer_id: block.proposer_id.to_string(),
                transaction_count: block.transactions.len(),
                state_hash: block.state_hash.to_string(),
                timestamp: format_unix_timestamp(block.timestamp),
            },
            transactions: block.transactions,
            signatures: block.signatures,
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new("Block not found", "not_found")),
        ))
    }
}

async fn get_block_by_hash(
    State(state): State<Arc<AppState>>,
    Path(hash_str): Path<String>,
) -> Result<Json<BlockDetail>, (StatusCode, Json<ApiError>)> {
    let bytes = hex::decode(hash_str.trim_start_matches("0x")).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Invalid hex", "invalid_param")),
        )
    })?;

    if bytes.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Invalid hash length", "invalid_param")),
        ));
    }

    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let hash = BlockHash(arr);

    let block = state.storage.get_block_by_hash(&hash).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    if let Some(block) = block {
        Ok(Json(BlockDetail {
            summary: BlockSummary {
                height: block.height,
                hash: hash.to_string(),
                parent_hash: block.parent_hash.to_string(),
                epoch: block.epoch,
                proposer_id: block.proposer_id.to_string(),
                transaction_count: block.transactions.len(),
                state_hash: block.state_hash.to_string(),
                timestamp: format_unix_timestamp(block.timestamp),
            },
            transactions: block.transactions,
            signatures: block.signatures,
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new("Block not found", "not_found")),
        ))
    }
}

// Accounts
#[derive(Serialize)]
struct AccountResponse {
    account_id: String,
    balance: u64,
    nonce: u64,
}

async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(account_id_str): Path<String>,
) -> Result<Json<AccountResponse>, (StatusCode, Json<ApiError>)> {
    let bytes = hex::decode(account_id_str.trim_start_matches("0x")).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Invalid hex", "invalid_param")),
        )
    })?;

    if bytes.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Invalid account id length", "invalid_param")),
        ));
    }

    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let account_id = AccountId(arr);

    let account = state.storage.get_account(&account_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    if let Some(account) = account {
        Ok(Json(AccountResponse {
            account_id: account_id.to_string(),
            balance: account.balance,
            nonce: account.nonce,
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new("Account not found", "not_found")),
        ))
    }
}

// Validators
#[derive(Serialize)]
struct ValidatorResponse {
    validator_id: String,
    voting_power: u64,
    account_id: String,
    active: bool,
}

async fn list_validators(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ValidatorResponse>>, (StatusCode, Json<ApiError>)> {
    let validators = state.storage.get_validators().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    let response = validators
        .into_iter()
        .map(|(id, v)| ValidatorResponse {
            validator_id: id.to_string(),
            voting_power: v.voting_power,
            account_id: v.account_id.to_string(),
            active: v.active,
        })
        .collect();

    Ok(Json(response))
}

/// Response for a connected peer.
#[derive(Serialize)]
struct PeerResponse {
    address: String,
    api_address: Option<String>,
    connected_since: String,
}

/// Lists all currently connected P2P peers.
async fn list_peers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PeerResponse>>, (StatusCode, Json<ApiError>)> {
    let map = state.peers.lock().map_err(|e| {
        tracing::error!("Peer map lock poisoned: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("Internal error", "lock_poisoned")),
        )
    })?;
    let peers = map
        .values()
        .map(|info| PeerResponse {
            address: info
                .api_address
                .map_or_else(|| info.address.to_string(), |a| a.to_string()),
            api_address: info.api_address.map(|a| a.to_string()),
            connected_since: format_unix_timestamp(info.connected_since),
        })
        .collect();
    Ok(Json(peers))
}

/// Formats a Unix timestamp (seconds) into an ISO 8601 UTC string.
fn format_unix_timestamp(secs: u64) -> String {
    let total_secs = secs;
    let days_since_epoch = total_secs / 86400;
    let time_of_day = total_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Converts days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let month_days: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }

    (y, (m as u64) + 1, remaining + 1)
}

/// Returns whether a year is a leap year.
fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// Transactions
#[derive(Serialize)]
struct SubmitTransactionResponse {
    tx_hash: String,
    status: String,
}

async fn submit_transaction(
    State(state): State<Arc<AppState>>,
    Json(tx): Json<Transaction>,
) -> Result<(StatusCode, Json<SubmitTransactionResponse>), (StatusCode, Json<ApiError>)> {
    let tx_bytes = serialize_transaction_canonical(&tx);
    if tx_bytes.len() > state.max_tx_bytes {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                format!(
                    "Transaction too large: {} bytes > {}",
                    tx_bytes.len(),
                    state.max_tx_bytes
                ),
                "tx_too_large",
            )),
        ));
    }

    // 1. Verify Signature
    if verify_transaction_signature(&tx).is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Invalid signature", "invalid_signature")),
        ));
    }

    // 2. Amount > 0
    if tx.amount == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Amount must be positive", "invalid_amount")),
        ));
    }

    // 3. Sender Exists and Balance/Nonce Check
    // We need current state to check balance/nonce
    // In a real high-throughput system we might check this against mempool + state,
    // but for V1 checking against committed state is safer/simpler (conservative).
    let sender_account = state.storage.get_account(&tx.sender).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(e.to_string(), "storage_error")),
        )
    })?;

    if let Some(account) = sender_account {
        if tx.nonce < account.nonce {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    format!(
                        "Invalid nonce: expected >= {}, got {}",
                        account.nonce, tx.nonce
                    ),
                    "invalid_nonce",
                )),
            ));
        }
        if account.balance < tx.amount {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    "Insufficient balance",
                    "insufficient_balance",
                )),
            ));
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Sender not found", "sender_not_found")),
        ));
    }

    // 4. Add to Mempool
    let mut mempool = state.mempool.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("Internal lock error", "internal_error")),
        )
    })?;
    match mempool.add(tx.clone()) {
        Ok(_) => {
            let hash = compute_transaction_hash(&tx);
            Ok((
                StatusCode::ACCEPTED,
                Json(SubmitTransactionResponse {
                    tx_hash: hash.to_string(),
                    status: "pending".to_string(),
                }),
            ))
        }
        Err(axiom_mempool::MempoolError::Duplicate) => {
            // Idempotent success
            let hash = compute_transaction_hash(&tx);
            Ok((
                StatusCode::ACCEPTED,
                Json(SubmitTransactionResponse {
                    tx_hash: hash.to_string(),
                    status: "pending".to_string(),
                }),
            ))
        }
        Err(axiom_mempool::MempoolError::Full) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("Mempool is full", "mempool_full")),
        )),
    }
}

pub fn app_router(state: Arc<AppState>, web_dir: PathBuf) -> Router {
    // API Routes (Hardening: Rate Limits, Timeouts, Tracing)
    let api_routes = Router::new()
        .route("/status", get(status))
        .route("/blocks", get(list_blocks))
        .route("/blocks/:height", get(get_block_by_height))
        .route("/blocks/by-hash/:hash", get(get_block_by_hash))
        .route("/accounts/:id", get(get_account))
        .route("/validators", get(list_validators))
        .route("/network/peers", get(list_peers))
        .route("/transactions", post(submit_transaction));

    Router::new()
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/auth/login", post(auth_login))
        .route("/auth/verify", post(auth_verify))
        .route("/auth/logout", post(auth_logout))
        .nest("/api", api_routes)
        .fallback_service(ServeDir::new(web_dir))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_error))
                .layer(BufferLayer::new(1024))
                .layer(TraceLayer::new_for_http())
                .layer(LoadShedLayer::new())
                .layer(RateLimitLayer::new(100, Duration::from_secs(1))) // 100 requests per second
                .layer(TimeoutLayer::new(Duration::from_secs(10))), // 10s timeout
        )
        .with_state(state)
}
