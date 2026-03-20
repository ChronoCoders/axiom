use axiom_api::{app_router, AppState};
use axiom_consensus::{construct_block, validate_and_commit_block};
use axiom_crypto::{compute_block_hash, sign_vote, verify_vote};
use axiom_execution::{compute_state_hash, select_fallback_proposer, select_proposer};
use axiom_mempool::Mempool;
use axiom_network::{Network, NetworkConfig, NetworkMessage};
use axiom_primitives::{
    Block, BlockHash, PublicKey, Signature, ValidatorId, ValidatorSignature, PROTOCOL_VERSION,
};
use axiom_storage::Storage;
use axum_server::tls_rustls::RustlsConfig;
use ed25519_dalek::SigningKey;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::genesis::load_genesis_state;

// NODE VERSION
const NODE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn start(config: AppConfig, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
    tracing::info!("Starting AXIOM Node: {}", config.node.node_id);
    tracing::info!("Node Version: {}", NODE_VERSION);
    tracing::info!("Protocol Version: {}", PROTOCOL_VERSION);
    tracing::info!("Data Directory: {:?}", config.node.data_dir);

    // Ensure data directory exists
    if let Err(e) = std::fs::create_dir_all(&config.node.data_dir) {
        tracing::error!("Failed to create data directory: {}", e);
        return;
    }

    // 3. Initialize Storage & State
    let storage = match Storage::initialize(&config.storage.sqlite_path) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("Failed to initialize storage: {e}");
            return;
        }
    };

    // Load latest state from DB or create genesis
    let latest_state = match storage.load_latest_state() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to load state: {e}");
            return;
        }
    };
    let (initial_state, height) = match latest_state {
        Some((s, h)) => {
            tracing::info!("Loaded state from disk (Height: {})", h);
            (s, h)
        }
        None => {
            tracing::info!(
                "No state found. Loading Genesis from file: {:?}",
                config.genesis.genesis_file
            );
            let genesis_state = match load_genesis_state(&config.genesis.genesis_file) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to load genesis file: {e}");
                    return;
                }
            };

            let hash = compute_state_hash(&genesis_state);
            tracing::info!("Computed Genesis Hash: {}", hex::encode(hash.0));

            // NOTE: Genesis hash check is done in main.rs (binary entry point).
            // Library consumers (tests) bypass strict checking or handle it themselves.

            // Store genesis state and hash (Meta)
            if let Err(e) = storage.store_genesis(&genesis_state, &hash) {
                tracing::error!("Failed to store genesis state: {e}");
                return;
            }

            // Commit genesis block to storage (Height 0)
            let genesis_block = Block {
                parent_hash: BlockHash([0; 32]),
                height: 0,
                epoch: 0,
                proposer_id: ValidatorId([0; 32]), // Null proposer for genesis
                transactions: vec![],
                signatures: vec![],
                state_hash: hash, // Use computed hash
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            if let Err(e) = storage.commit_block(&genesis_block, &genesis_state) {
                tracing::error!("Failed to commit genesis: {e}");
                return;
            }
            tracing::info!("Genesis committed to storage.");

            (genesis_state, 0)
        }
    };

    let state = Arc::new(Mutex::new(initial_state));
    let current_height = Arc::new(AtomicUsize::new(height as usize));

    // 4. Initialize Mempool
    let mempool_capacity = match usize::try_from(config.mempool.max_size) {
        Ok(v) if v > 0 => v,
        _ => {
            tracing::error!("Invalid mempool.max_size: {}", config.mempool.max_size);
            return;
        }
    };
    let max_tx_bytes = match usize::try_from(config.mempool.max_tx_bytes) {
        Ok(v) if v > 0 => v,
        _ => {
            tracing::error!(
                "Invalid mempool.max_tx_bytes: {}",
                config.mempool.max_tx_bytes
            );
            return;
        }
    };

    let mempool = Arc::new(Mutex::new(Mempool::new(mempool_capacity)));

    // 5. Initialize Network
    let bind_addr = match config.network.listen_address.parse() {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(
                "Invalid listen address '{}': {e}",
                config.network.listen_address
            );
            return;
        }
    };
    let mut peer_addrs = Vec::new();
    for s in config.network.peers.clone().unwrap_or_default() {
        match s.parse() {
            Ok(addr) => peer_addrs.push(addr),
            Err(e) => {
                tracing::error!("Invalid peer address '{s}': {e}");
                return;
            }
        }
    }
    let mut peer_api_map = std::collections::HashMap::new();
    if let Some(ref api_map) = config.network.peer_api_map {
        for (p2p_str, api_str) in api_map {
            match (
                p2p_str.parse::<std::net::SocketAddr>(),
                api_str.parse::<std::net::SocketAddr>(),
            ) {
                (Ok(p2p), Ok(api)) => {
                    peer_api_map.insert(p2p, api);
                }
                (Err(e), _) => {
                    tracing::warn!("Invalid P2P address in peer_api_map '{}': {}", p2p_str, e);
                }
                (_, Err(e)) => {
                    tracing::warn!("Invalid API address in peer_api_map '{}': {}", api_str, e);
                }
            }
        }
    }

    let genesis_hash = match storage.get_genesis_hash() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to load genesis hash for network identity: {e}");
            return;
        }
    };
    let net_config = NetworkConfig {
        bind_addr,
        peers: peer_addrs,
        retry_interval: None,
        peer_api_map,
        local_height: height,
        local_genesis_hash: genesis_hash,
        local_protocol_version: PROTOCOL_VERSION,
    };

    let (net_tx, mut net_rx, peer_map) =
        Network::start(net_config, shutdown_rx.resubscribe()).await;

    // Create a broadcast channel for internal shutdown signals (e.g. from API)
    let mut shutdown_rx_api = shutdown_rx.resubscribe();

    // 6. Initialize API
    let app_state = Arc::new(AppState {
        storage: storage.clone(),
        mempool: mempool.clone(),
        peers: peer_map,
        auth_tokens: Arc::new(RwLock::new(HashSet::new())),
        console_user: config.console.user.clone(),
        console_pass: config.console.password.clone(),
        token_counter: AtomicU64::new(0),
        max_tx_bytes,
    });

    let api_addr = match config.api.bind_address.parse::<std::net::SocketAddr>() {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(
                "Invalid API bind address '{}': {e}",
                config.api.bind_address
            );
            return;
        }
    };

    let mut web_dir = std::path::PathBuf::from("web");
    if !web_dir.exists() {
        // Try parent directory if we are in node/
        let parent_web = std::path::PathBuf::from("../web");
        if parent_web.exists() {
            web_dir = parent_web;
        } else {
            tracing::warn!(
                "Web directory not found at {:?} or ../web. Console UI may not work.",
                web_dir
            );
        }
    }

    if let Ok(abs_path) = web_dir.canonicalize() {
        tracing::info!("Serving web files from: {:?}", abs_path);
        web_dir = abs_path;
    } else {
        tracing::info!("Serving web files from: {:?} (relative)", web_dir);
    }

    let router = app_router(app_state, web_dir);

    let api_config = config.api.clone();

    tokio::spawn(async move {
        if api_config.tls_enabled {
            let cert_path = match api_config.tls_cert_path {
                Some(p) => p,
                None => {
                    tracing::error!("TLS enabled but no cert path configured");
                    return;
                }
            };
            let key_path = match api_config.tls_key_path {
                Some(p) => p,
                None => {
                    tracing::error!("TLS enabled but no key path configured");
                    return;
                }
            };

            // Check if files exist
            if !cert_path.exists() || !key_path.exists() {
                tracing::error!(
                    "TLS cert/key not found. Cert: {:?}, Key: {:?}",
                    cert_path,
                    key_path
                );
                return;
            }

            let tls_config = match RustlsConfig::from_pem_file(cert_path, key_path).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to load TLS certs: {}", e);
                    return;
                }
            };

            let listener = match std::net::TcpListener::bind(api_addr) {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind API address: {e}");
                    return;
                }
            };
            if let Err(e) = listener.set_nonblocking(true) {
                tracing::error!("Failed to set non-blocking: {e}");
                return;
            }
            let local_addr = match listener.local_addr() {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("Failed to get local address: {e}");
                    return;
                }
            };
            tracing::info!("Starting API server with TLS on https://{}", local_addr);
            tracing::info!("Console UI available at https://{}/", local_addr);

            let handle = axum_server::Handle::new();

            // Spawn a task to listen for shutdown and trigger handle
            let handle_clone = handle.clone();
            tokio::spawn(async move {
                let _ = shutdown_rx_api.recv().await;
                tracing::info!("API server shutting down (TLS)...");
                handle_clone.graceful_shutdown(Some(Duration::from_secs(5)));
            });

            if let Err(e) = axum_server::from_tcp_rustls(listener, tls_config)
                .handle(handle)
                .serve(router.into_make_service())
                .await
            {
                tracing::error!("API Server Error: {}", e);
            }
        } else {
            let listener = match tokio::net::TcpListener::bind(api_addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind API address: {e}");
                    return;
                }
            };
            let local_addr = match listener.local_addr() {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("Failed to get local address: {e}");
                    return;
                }
            };
            tracing::info!("Starting API server on http://{}", local_addr);
            tracing::info!("Console UI available at http://{}/", local_addr);

            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx_api.recv().await;
                    tracing::info!("API server shutting down...");
                })
                .await
            {
                tracing::error!("API Server Error: {}", e);
            }
        }
    });

    // 7. Consensus Loop

    // Load Validator Key: check config (programmatic/test use) first, then env var (CODING_RULES 5.3)
    let validator_key_hex = config
        .validator
        .private_key
        .clone()
        .or_else(|| std::env::var("AXIOM_VALIDATOR_PRIVATE_KEY").ok());
    let (my_validator_id, my_private_key) = if let Some(key_hex) = validator_key_hex {
        let bytes = match hex::decode(&key_hex) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Invalid validator private key hex: {e}");
                return;
            }
        };
        let bytes_arr: [u8; 32] = match bytes.try_into() {
            Ok(a) => a,
            Err(_) => {
                tracing::error!("Invalid validator private key length (expected 32 bytes)");
                return;
            }
        };
        let sk = SigningKey::from_bytes(&bytes_arr);
        let pk = sk.verifying_key();
        let val_id = ValidatorId(pk.to_bytes());
        tracing::info!("Validator Mode Active. ID: {}", val_id);
        (Some(val_id), Some(sk))
    } else {
        tracing::info!("Observer Mode Active (No AXIOM_VALIDATOR_PRIVATE_KEY env var set).");
        (None, None)
    };

    // Allow network to stabilize before starting consensus
    // Nodes need time to establish peer connections (reconnect interval is 5s)
    tokio::time::sleep(Duration::from_secs(6)).await;

    tracing::info!("Starting Consensus Loop...");

    // State for Consensus
    // We keep track of votes for each block hash at each height
    let mut vote_pool: HashMap<(u64, BlockHash), Vec<ValidatorSignature>> = HashMap::new();
    // Pending block: We have validated it, but we are waiting for quorum.
    let mut pending_block: Option<Block> = None;
    let mut pending_block_since: Option<std::time::Instant> = None;

    // RESTART RECOVERY: Load pending block and replay vote if it exists
    {
        let height = current_height.load(std::sync::atomic::Ordering::SeqCst) as u64;
        let next_height = height + 1;

        match storage.get_pending_blocks_by_height(next_height) {
            Ok(blocks) => {
                if let Some(block) = blocks.first() {
                    tracing::info!(
                        "Loaded pending block from storage (Restart Recovery): {}",
                        next_height
                    );
                    pending_block = Some(block.clone());
                    pending_block_since = Some(std::time::Instant::now());

                    // VOTE REPLAY: Check for persisted vote
                    if let (Some(val_id), Some(_)) = (&my_validator_id, &my_private_key) {
                        let block_hash = compute_block_hash(block);
                        if let Ok(Some((stored_hash, stored_sig_str))) =
                            storage.get_own_vote(next_height)
                        {
                            if stored_hash == block_hash {
                                if let Ok(sig_bytes) = hex::decode(&stored_sig_str) {
                                    if sig_bytes.len() == 64 {
                                        let mut arr = [0u8; 64];
                                        arr.copy_from_slice(&sig_bytes);
                                        let signature = Signature(arr);

                                        let sig_struct = ValidatorSignature {
                                            validator_id: *val_id,
                                            signature,
                                        };

                                        // 1. Inject into vote_pool
                                        vote_pool
                                            .entry((next_height, block_hash))
                                            .or_default()
                                            .push(sig_struct.clone());

                                        // Wait for network to stabilize (ensure peers are connected so they receive the vote)
                                        tracing::info!("Waiting for network stabilization before broadcasting vote...");
                                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                                        // 2. Broadcast exactly once
                                        let vote_msg = NetworkMessage::Vote(
                                            sig_struct,
                                            block_hash,
                                            next_height,
                                        );
                                        if let Err(e) = net_tx.send(vote_msg).await {
                                            tracing::error!("Failed to replay vote: {}", e);
                                        }

                                        tracing::info!(
                                            "Replayed persisted vote for pending block {}",
                                            next_height
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => tracing::error!("Failed to check pending blocks on startup: {}", e),
        }
    }

    let mut proposal_attempt: u64 = 0;
    let mut last_proposal_time = std::time::Instant::now();
    let proposal_timeout = Duration::from_secs(5);
    let max_pending_timeout = Duration::from_secs(30);
    let mut last_height_seen = 0;

    loop {
        if shutdown_rx.try_recv().is_ok() {
            tracing::info!("Consensus loop shutting down...");
            break;
        }

        let height = current_height.load(std::sync::atomic::Ordering::SeqCst) as u64;
        let next_height = height + 1;

        // REMOVED: In-loop pending block loading (moved to initialization)

        tracing::info!(
            height = height,
            attempt = proposal_attempt,
            pending = pending_block.is_some(),
            "Consensus Loop Start"
        );

        // Reset timeout tracking when height advances
        if height != last_height_seen {
            proposal_attempt = 0;
            last_proposal_time = std::time::Instant::now();
            pending_block_since = None;
            last_height_seen = height;
        }

        // ---------------------------------------------------------------------
        // 1. Proposer Logic
        // ---------------------------------------------------------------------
        if pending_block.is_none() {
            let elapsed = last_proposal_time.elapsed();
            let is_timeout = elapsed > proposal_timeout;

            let mut should_propose = false;

            if let (Some(val_id), Some(sk)) = (&my_validator_id, &my_private_key) {
                let state_guard = match state.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        tracing::error!("State lock poisoned: {e}");
                        break;
                    }
                };
                if proposal_attempt == 0 {
                    // Primary proposer check
                    match select_proposer(&state_guard, next_height) {
                        Ok(proposer) => should_propose = proposer == *val_id,
                        Err(e) => {
                            tracing::error!("Failed to select proposer: {}", e);
                        }
                    }
                } else {
                    // Fallback proposer check
                    match select_fallback_proposer(&state_guard, next_height, proposal_attempt) {
                        Ok(fallback) => should_propose = fallback == *val_id,
                        Err(e) => {
                            tracing::error!("Failed to select fallback proposer: {}", e);
                        }
                    }
                }

                if should_propose {
                    tracing::info!(
                        "We are the proposer for height {} (Attempt {})!",
                        next_height,
                        proposal_attempt
                    );

                    // Get parent block
                    let parent_block = match storage.get_block_by_height(height) {
                        Ok(Some(b)) => b,
                        Ok(None) => {
                            tracing::error!("Missing parent block at height {height}");
                            continue;
                        }
                        Err(e) => {
                            tracing::error!("Failed to get parent block: {e}");
                            continue;
                        }
                    };
                    let parent_hash = compute_block_hash(&parent_block);

                    // Get txs from mempool
                    let mempool_guard = match mempool.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::error!("Mempool lock poisoned: {e}");
                            continue;
                        }
                    };
                    let txs = mempool_guard.get_batch(1000); // MAX_TX
                    drop(mempool_guard);

                    // Construct
                    // REMOVED: let state_guard = state.lock().unwrap(); // Avoid deadlock, reuse outer guard
                    match construct_block(&state_guard, next_height, parent_hash, txs, sk, val_id) {
                        Ok(mut block) => {
                            block.timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            tracing::info!("Proposing block at height {}", next_height);

                            let block_hash = compute_block_hash(&block);

                            if let Err(e) = storage.save_pending_block(&block) {
                                tracing::error!("Failed to save pending block: {}", e);
                            }

                            // Broadcast Proposal
                            let net_tx_clone = net_tx.clone();
                            let block_clone = block.clone();
                            tokio::spawn(async move {
                                if let Err(e) = net_tx_clone
                                    .send(NetworkMessage::BlockProposal(block_clone))
                                    .await
                                {
                                    tracing::error!("Failed to broadcast block: {}", e);
                                }
                            });

                            // Broadcast our Vote
                            // construct_block already adds our signature to block.signatures
                            if let Some(sig) = block.signatures.first() {
                                let sig_str = hex::encode(sig.signature.0);
                                if let Err(e) = storage.save_own_vote(next_height, &block_hash, &sig_str) {
                                    tracing::error!("Failed to save own vote: {}", e);
                                }

                                let vote_msg =
                                    NetworkMessage::Vote(sig.clone(), block_hash, next_height);
                                let net_tx_clone2 = net_tx.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = net_tx_clone2.send(vote_msg).await {
                                        tracing::error!("Failed to broadcast vote: {}", e);
                                    }
                                });

                                // Add our vote to pool
                                let votes = vote_pool.entry((next_height, block_hash)).or_default();
                                if !votes.iter().any(|v| v.validator_id == sig.validator_id) {
                                    votes.push(sig.clone());
                                }
                            }

                            // Store as pending
                            pending_block = Some(block);
                            pending_block_since = Some(std::time::Instant::now());
                        }
                        Err(e) => tracing::error!("Failed to construct block: {}", e),
                    }
                }
            }

            // Timeout logic: Advance attempt if we didn't propose (and timeout elapsed)
            if pending_block.is_none() && is_timeout {
                tracing::warn!(
                    "Proposal timeout at height {}, attempt {}",
                    next_height,
                    proposal_attempt
                );
                proposal_attempt += 1;
                last_proposal_time = std::time::Instant::now();
            }
        }

        // ---------------------------------------------------------------------
        // 2. Check Quorum & Commit
        // ---------------------------------------------------------------------
        let mut committed = false;
        if let Some(block) = &pending_block {
            if block.height == next_height {
                let block_hash = compute_block_hash(block);
                if let Some(votes) = vote_pool.get(&(next_height, block_hash)) {
                    // Calculate voting power
                    let mut state_guard = match state.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::error!("State lock poisoned: {e}");
                            break;
                        }
                    };
                    let total_power = match state_guard.total_voting_power() {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("Voting power overflow: {e}. Skipping quorum check.");
                            continue;
                        }
                    };
                    let mut collected_power: u64 = 0;
                    let mut unique_validators = HashSet::new();

                    for vote in votes {
                        if unique_validators.insert(vote.validator_id) {
                            if let Some(val) = state_guard.get_validator(&vote.validator_id) {
                                if val.active {
                                    collected_power += val.voting_power;
                                }
                            }
                        }
                    }

                    // Quorum: > 2/3
                    if collected_power * 3 > total_power * 2 {
                        tracing::info!(
                            "Quorum reached for height {} (Power: {}/{})",
                            next_height,
                            collected_power,
                            total_power
                        );

                        // Aggregate signatures into block
                        let mut final_block = block.clone();
                        final_block.signatures = votes.clone();

                        // Get parent hash again
                        let parent_block = match storage.get_block_by_height(height) {
                            Ok(Some(b)) => b,
                            Ok(None) => {
                                tracing::error!("Missing parent block at height {height}");
                                continue;
                            }
                            Err(e) => {
                                tracing::error!("Failed to get parent block: {e}");
                                continue;
                            }
                        };
                        let parent_hash = compute_block_hash(&parent_block);

                        // Commit
                        match validate_and_commit_block(
                            &state_guard,
                            &final_block,
                            &parent_hash,
                            height,
                        ) {
                            Ok(new_state) => {
                                tracing::info!(
                                    "Block {} committed successfully.",
                                    final_block.height
                                );
                                match storage.commit_block(&final_block, &new_state) {
                                    Ok(_) => {
                                        // Update State
                                        *state_guard = new_state;

                                        // Update Mempool
                                        let mut mempool_guard = match mempool.lock() {
                                            Ok(g) => g,
                                            Err(e) => {
                                                tracing::error!("Mempool lock poisoned: {e}");
                                                break;
                                            }
                                        };
                                        let tx_hashes: Vec<_> = final_block
                                            .transactions
                                            .iter()
                                            .map(axiom_crypto::compute_transaction_hash)
                                            .collect();
                                        mempool_guard.remove_batch(&tx_hashes);

                                        committed = true;
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to commit block to storage: {}", e);
                                        if let Ok(Some((loaded_state, loaded_height))) =
                                            storage.load_latest_state()
                                        {
                                            if loaded_height >= next_height {
                                                tracing::warn!("Storage is ahead (Height: {}). Updating state.", loaded_height);
                                                *state_guard = loaded_state;
                                                current_height.store(
                                                    loaded_height as usize,
                                                    std::sync::atomic::Ordering::SeqCst,
                                                );
                                                pending_block = None;
                                                pending_block_since = None;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => tracing::error!("Failed to commit quorum block: {}", e),
                        }
                    }
                }
            }
        }

        // Stale block cleanup
        if !committed && pending_block.is_some() {
            let exceeded_hard_timeout = pending_block_since
                .map(|t| t.elapsed() > max_pending_timeout)
                .unwrap_or(false);
            let exceeded_soft_timeout = last_proposal_time.elapsed() > proposal_timeout * 6;

            if exceeded_hard_timeout || exceeded_soft_timeout {
                let block = match pending_block.as_ref() {
                    Some(b) => b,
                    None => continue,
                };
                let block_hash = compute_block_hash(block);
                let has_votes = vote_pool
                    .get(&(next_height, block_hash))
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);

                if exceeded_hard_timeout {
                    tracing::warn!(
                        "Clearing stale pending block at height {} (hard timeout {}s exceeded, votes={})",
                        next_height,
                        max_pending_timeout.as_secs(),
                        has_votes,
                    );
                    if let Err(e) = storage.mark_pending_blocks_inactive(next_height) {
                        tracing::error!("Failed to mark stale blocks inactive: {}", e);
                    }
                    pending_block = None;
                    pending_block_since = None;
                    last_proposal_time = std::time::Instant::now();
                } else if has_votes {
                    tracing::info!(
                        "Pending block at height {} has votes, extending timeout...",
                        next_height
                    );
                    last_proposal_time = std::time::Instant::now();
                } else {
                    tracing::warn!(
                        "Clearing stale pending block at height {} (no quorum after {}s)",
                        next_height,
                        proposal_timeout.as_secs() * 6
                    );
                    if let Err(e) = storage.mark_pending_blocks_inactive(next_height) {
                        tracing::error!("Failed to mark stale blocks inactive: {}", e);
                    }
                    pending_block = None;
                    pending_block_since = None;
                    last_proposal_time = std::time::Instant::now();
                }
            }
        }

        if committed {
            // Mark pending blocks inactive (Durability Req 4)
            if let Err(e) = storage.mark_pending_blocks_inactive(next_height) {
                tracing::error!("Failed to mark committed blocks inactive: {}", e);
            }
            pending_block = None;
            pending_block_since = None;
            current_height.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Clean up old votes
            vote_pool.retain(|(h, _), _| *h >= next_height);
            // Brief yield to allow shutdown checks and prevent tight loops
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue; // Skip wait, go to next height
        }

        // ---------------------------------------------------------------------
        // 3. Handle Messages
        // ---------------------------------------------------------------------
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("Consensus loop shutting down...");
                break;
            }
            msg_opt = net_rx.recv() => {
                if let Some(msg) = msg_opt {
                    match msg {
                        NetworkMessage::BlockProposal(block) => {
                            tracing::info!("Received Block Proposal: Height {}", block.height);
                            if block.height <= height {
                                tracing::debug!("Ignoring old block");
                                continue;
                            }
                            // Future Block Handling (Durability Req 5)
                            if block.height > next_height {
                                tracing::warn!("Future block received (Height {}). Buffering.", block.height);
                                if let Err(e) = storage.save_pending_block(&block) {
                                    tracing::error!("Failed to buffer future block: {}", e);
                                }
                                continue;
                            }
                            if pending_block.is_some() {
                                // Check if incoming block has quorum (committed block from advanced peer)
                                let dominated = {
                                    let sg = match state.lock() {
                                        Ok(g) => g,
                                        Err(e) => {
                                            tracing::error!("State lock poisoned: {e}");
                                            break;
                                        }
                                    };
                                    let total_power: u64 = match sg.total_voting_power() {
                                        Ok(p) => p,
                                        Err(_) => {
                                            tracing::error!("Voting power overflow during block validation, rejecting block");
                                            u64::MAX
                                        }
                                    };
                                    let block_hash = compute_block_hash(&block);
                                    let mut collected_power: u64 = 0;
                                    let mut unique_validators = HashSet::new();

                                    for sig in &block.signatures {
                                        if unique_validators.insert(sig.validator_id) {
                                            let public_key = PublicKey(sig.validator_id.0);
                                            // Verify signature and validator existence/activity
                                            if verify_vote(&public_key, &block_hash, block.height, &sig.signature).is_ok() {
                                                if let Some(val) = sg.get_validator(&sig.validator_id) {
                                                    if val.active {
                                                        collected_power += val.voting_power;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    collected_power * 3 > total_power * 2
                                };

                                if dominated {
                                    tracing::info!(
                                        "Replacing stale pending block with committed block at height {}",
                                        block.height
                                    );
                                    // Clear old vote_pool entries for this height
                                    vote_pool.retain(|&(h, _), _| h != block.height);
                                    pending_block = None;
                                    pending_block_since = None;
                                    // Fall through to normal acceptance logic below
                                } else {
                                    tracing::debug!("Already have pending block for height {}, ignoring proposal", block.height);
                                    continue;
                                }
                            }
                            let block_hash = compute_block_hash(&block);
                            for sig in &block.signatures {
                                let public_key = PublicKey(sig.validator_id.0);
                                if verify_vote(&public_key, &block_hash, block.height, &sig.signature).is_ok() {
                                    let votes = vote_pool.entry((block.height, block_hash)).or_default();
                                    if !votes.iter().any(|v| v.validator_id == sig.validator_id) {
                                        votes.push(sig.clone());
                                    }
                                }
                            }
                            {
                                let state_guard = match state.lock() {
                                    Ok(g) => g,
                                    Err(e) => {
                                        tracing::error!("State lock poisoned: {e}");
                                        break;
                                    }
                                };
                                match state_guard.get_validator(&block.proposer_id) {
                                    Some(v) if v.active => {}
                                    _ => {
                                        tracing::warn!("Block proposal from unknown/inactive validator: {}", block.proposer_id);
                                        continue;
                                    }
                                }
                            }

                            // Persist block before voting (Durability Req 1)
                            if let Err(e) = storage.save_pending_block(&block) {
                                tracing::error!("Failed to persist pending block: {}. Cannot vote.", e);
                                continue;
                            }

                            if let (Some(val_id), Some(sk)) = (&my_validator_id, &my_private_key) {
                                let block_hash = compute_block_hash(&block);

                                // Check for existing vote (Durability Req 2)
                                let mut signature: Option<Signature> = None;
                                let mut reuse_vote = false;

                                match storage.get_own_vote(block.height) {
                                    Ok(Some((stored_hash, stored_sig_str))) => {
                                        if stored_hash == block_hash {
                                            tracing::info!("Reusing existing vote for block {}", block_hash);
                                            // Parse stored_sig_str back to Signature
                                            if let Ok(bytes) = hex::decode(&stored_sig_str) {
                                                if bytes.len() == 64 {
                                                    let mut arr = [0u8; 64];
                                                    arr.copy_from_slice(&bytes);
                                                    signature = Some(Signature(arr));
                                                    reuse_vote = true;
                                                }
                                            }
                                            if !reuse_vote {
                                                tracing::error!("Failed to parse stored signature for reuse");
                                                continue;
                                            }
                                        } else {
                                            tracing::error!("CRITICAL: Attempted to double sign at height {}! (Old: {}, New: {})", block.height, stored_hash, block_hash);
                                            continue; // Do not vote
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to check existing votes: {}", e);
                                        continue;
                                    }
                                    Ok(None) => {}
                                }

                                if !reuse_vote {
                                    tracing::info!("Voting for block at height {}", block.height);
                                    let sig = sign_vote(sk, &block_hash, block.height);
                                    let sig_str = hex::encode(sig.0);

                                    // Persist vote (Durability Req 2)
                                    if let Err(e) = storage.save_own_vote(block.height, &block_hash, &sig_str) {
                                        tracing::error!("Failed to persist vote: {}. Cannot broadcast.", e);
                                        continue;
                                    }
                                    signature = Some(sig);
                                }

                                let final_sig = match signature {
                                    Some(s) => s,
                                    None => {
                                        tracing::error!("Signature not set after vote logic");
                                        continue;
                                    }
                                };
                                let sig_struct = ValidatorSignature {
                                    validator_id: *val_id,
                                    signature: final_sig,
                                };
                                let vote_msg = NetworkMessage::Vote(sig_struct.clone(), block_hash, block.height);
                                let net_tx_clone = net_tx.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = net_tx_clone.send(vote_msg).await {
                                        tracing::error!("Failed to broadcast vote: {}", e);
                                    }
                                });
                                let votes = vote_pool.entry((block.height, block_hash)).or_default();
                                if !votes.iter().any(|v| v.validator_id == sig_struct.validator_id) {
                                    votes.push(sig_struct);
                                }
                            }
                            pending_block = Some(block);
                            if pending_block_since.is_none() {
                                pending_block_since = Some(std::time::Instant::now());
                            }
                        }
                        NetworkMessage::Vote(sig, block_hash, vote_height) => {
                            tracing::info!("Received Vote for height {}", vote_height);
                            tracing::info!(height = vote_height, validator = %sig.validator_id, "Received Vote");

                            if vote_height < next_height {
                                continue;
                            }

                            // Verify Signature
                            let public_key = PublicKey(sig.validator_id.0);
                            if let Err(e) = verify_vote(&public_key, &block_hash, vote_height, &sig.signature) {
                                tracing::warn!("Invalid vote signature: {}", e);
                                continue;
                            }

                            // Add to pool
                            let votes = vote_pool.entry((vote_height, block_hash)).or_default();
                            if !votes.iter().any(|v| v.validator_id == sig.validator_id) {
                                votes.push(sig);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Periodic tick
            }
        }
    }

    tracing::info!("Node Shutdown Complete.");
}
