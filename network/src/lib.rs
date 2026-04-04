#![deny(warnings)]

use axiom_primitives::Block;
use axiom_primitives::BlockHash;
use axiom_primitives::Evidence;
use axiom_primitives::Proposal;
use axiom_primitives::StateHash;
use axiom_primitives::Transaction;
use axiom_primitives::ValidatorSignature;
use axiom_primitives::Vote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

const DEFAULT_MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_TX_BYTES: usize = 65_536;
const DEFAULT_MAX_BLOCK_BYTES: usize = 2 * 1_048_576;
const DEFAULT_MAX_EVIDENCE_BYTES: usize = 131_072;
const DEFAULT_MAX_MESSAGES_PER_SEC: u32 = 200;
const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_HANDSHAKE_MESSAGES: u32 = 32;

#[derive(Clone, Copy)]
struct ConnectionLimits {
    max_message_bytes: usize,
    max_tx_bytes: usize,
    max_block_bytes: usize,
    max_evidence_bytes: usize,
    max_messages_per_sec: u32,
    handshake_timeout: Duration,
    max_handshake_messages: u32,
}

impl ConnectionLimits {
    fn normalized(self) -> Self {
        Self {
            max_message_bytes: if self.max_message_bytes == 0 {
                DEFAULT_MAX_MESSAGE_BYTES
            } else {
                self.max_message_bytes
            },
            max_tx_bytes: if self.max_tx_bytes == 0 {
                DEFAULT_MAX_TX_BYTES
            } else {
                self.max_tx_bytes
            },
            max_block_bytes: if self.max_block_bytes == 0 {
                DEFAULT_MAX_BLOCK_BYTES
            } else {
                self.max_block_bytes
            },
            max_evidence_bytes: if self.max_evidence_bytes == 0 {
                DEFAULT_MAX_EVIDENCE_BYTES
            } else {
                self.max_evidence_bytes
            },
            max_messages_per_sec: if self.max_messages_per_sec == 0 {
                DEFAULT_MAX_MESSAGES_PER_SEC
            } else {
                self.max_messages_per_sec
            },
            handshake_timeout: if self.handshake_timeout == Duration::from_secs(0) {
                DEFAULT_HANDSHAKE_TIMEOUT
            } else {
                self.handshake_timeout
            },
            max_handshake_messages: if self.max_handshake_messages == 0 {
                DEFAULT_MAX_HANDSHAKE_MESSAGES
            } else {
                self.max_handshake_messages
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NetworkMessage {
    BlockProposal(Block),
    Vote(ValidatorSignature, BlockHash, u64), // sig, block_hash, height
    TransactionGossip(Transaction),
    Proposal(Proposal),
    ConsensusVote(Vote),
    Evidence(Evidence),
    StatusRequest,
    StatusResponse {
        protocol_version: u64,
        height: u64,
        genesis_hash: StateHash,
    },
}

/// Configuration for the P2P network layer.
pub struct NetworkConfig {
    /// Address to listen on for incoming connections.
    pub bind_addr: SocketAddr,
    /// List of peer addresses to connect to.
    pub peers: Vec<SocketAddr>,
    /// Retry interval for reconnecting to peers.
    pub retry_interval: Option<Duration>,
    /// Optional mapping from P2P peer address to that peer's API address.
    pub peer_api_map: HashMap<SocketAddr, SocketAddr>,
    /// Local node's current committed height (best-effort; used only for status handshake).
    pub local_height: u64,
    /// Local node's genesis hash (network identity).
    pub local_genesis_hash: StateHash,
    /// Local node's protocol version (network identity).
    pub local_protocol_version: u64,
    /// Maximum size of any single framed network message in bytes (length prefix excludes).
    pub max_message_bytes: usize,
    /// Maximum size of a gossiped transaction payload.
    pub max_tx_bytes: usize,
    /// Maximum size of a gossiped block/proposal payload.
    pub max_block_bytes: usize,
    /// Maximum size of a gossiped evidence payload.
    pub max_evidence_bytes: usize,
    /// Maximum number of messages allowed per second per connection.
    pub max_messages_per_sec: u32,
    /// Maximum time allowed for a peer to complete the status handshake.
    pub handshake_timeout: Duration,
    /// Maximum number of non-handshake messages tolerated before handshake completes.
    pub max_handshake_messages: u32,
}

/// Information about a connected peer.
#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    /// The socket address of the peer (P2P port).
    pub address: SocketAddr,
    /// The API address of the peer, if known.
    pub api_address: Option<SocketAddr>,
    /// Unix timestamp (seconds) when the connection was established.
    pub connected_since: u64,
}

/// Shared map of currently connected peers keyed by their address.
pub type PeerMap = Arc<Mutex<HashMap<SocketAddr, PeerInfo>>>;

pub struct Network;

impl Network {
    /// Starts the network layer with persistent connections and reconnection logic.
    /// Returns (sender_to_network, receiver_from_network, peer_map)
    pub async fn start(
        config: NetworkConfig,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> (
        mpsc::Sender<NetworkMessage>,
        mpsc::Receiver<NetworkMessage>,
        PeerMap,
    ) {
        // Channel for Node to send messages to Network (to be broadcasted)
        let (net_tx, mut net_rx) = mpsc::channel::<NetworkMessage>(1000);

        // Channel for Network to send messages to Node (received from peers)
        let (node_tx, node_rx) = mpsc::channel::<NetworkMessage>(1000);

        // Broadcast channel to distribute messages to all peer tasks
        // We use a broadcast channel so we can fan-out the single message from net_rx to all peers
        let (bcast_tx, _) = broadcast::channel::<NetworkMessage>(1000);

        // 1. Outgoing Message Distributor (Fan-out)
        let bcast_tx_clone = bcast_tx.clone();
        let mut shutdown_rx_dist = shutdown_rx.resubscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = net_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                let _ = bcast_tx_clone.send(msg);
                            }
                            None => break,
                        }
                    }
                    _ = shutdown_rx_dist.recv() => {
                        break;
                    }
                }
            }
        });

        let retry_interval = config.retry_interval.unwrap_or(Duration::from_secs(5));
        let peer_map: PeerMap = Arc::new(Mutex::new(HashMap::new()));

        // 2. Peer Connection Tasks (Persistent Outgoing)
        // For each peer in the config, we start a dedicated task that maintains a connection
        let limits = ConnectionLimits {
            max_message_bytes: config.max_message_bytes,
            max_tx_bytes: config.max_tx_bytes,
            max_block_bytes: config.max_block_bytes,
            max_evidence_bytes: config.max_evidence_bytes,
            max_messages_per_sec: config.max_messages_per_sec,
            handshake_timeout: config.handshake_timeout,
            max_handshake_messages: config.max_handshake_messages,
        }
        .normalized();

        for peer_addr in config.peers {
            let bcast_rx = bcast_tx.subscribe();
            let mut shutdown_rx_peer = shutdown_rx.resubscribe();
            let peer_map_clone = Arc::clone(&peer_map);
            let api_addr = config.peer_api_map.get(&peer_addr).copied();
            let local_height = config.local_height;
            let local_genesis_hash = config.local_genesis_hash;
            let local_protocol_version = config.local_protocol_version;

            tokio::spawn(async move {
                info!("Starting connection manager for peer {}", peer_addr);
                loop {
                    // Try to connect
                    let connect_fut = TcpStream::connect(peer_addr);

                    let mut stream = tokio::select! {
                         res = connect_fut => {
                             match res {
                                 Ok(s) => s,
                                 Err(e) => {
                                     debug!("Failed to connect to {}: {}. Retrying...", peer_addr, e);
                                     tokio::select! {
                                         _ = tokio::time::sleep(retry_interval) => continue,
                                         _ = shutdown_rx_peer.recv() => return,
                                     }
                                 }
                             }
                         }
                         _ = shutdown_rx_peer.recv() => return,
                    };

                    info!("Connected to peer {}", peer_addr);

                    if let Err(e) = perform_handshake(
                        &mut stream,
                        local_protocol_version,
                        local_height,
                        local_genesis_hash,
                        limits,
                    )
                    .await
                    {
                        warn!(
                            "Handshake failed with {}: {}. Reconnecting...",
                            peer_addr, e
                        );
                        tokio::select! {
                            _ = tokio::time::sleep(retry_interval) => {},
                            _ = shutdown_rx_peer.recv() => return,
                        }
                        continue;
                    }

                    let now =
                        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                            Ok(d) => d.as_secs(),
                            Err(e) => {
                                warn!("System clock before Unix epoch: {}", e);
                                0
                            }
                        };
                    if let Ok(mut map) = peer_map_clone.lock() {
                        map.insert(
                            peer_addr,
                            PeerInfo {
                                address: peer_addr,
                                api_address: api_addr,
                                connected_since: now,
                            },
                        );
                    }

                    let mut rx = bcast_rx.resubscribe();

                    loop {
                        tokio::select! {
                            msg_res = rx.recv() => {
                                match msg_res {
                                    Ok(msg) => {
                                         if matches!(msg, NetworkMessage::StatusRequest | NetworkMessage::StatusResponse { .. }) {
                                            continue;
                                         }
                                         let bytes = match rmp_serde::to_vec(&msg) {
                                            Ok(b) => b,
                                            Err(e) => {
                                                error!("Serialization error: {}", e);
                                                continue;
                                            }
                                         };

                                         if bytes.len() > limits.max_message_bytes {
                                             warn!(
                                                 "Dropping outbound message to {} ({} bytes > max {})",
                                                 peer_addr,
                                                 bytes.len(),
                                                 limits.max_message_bytes
                                             );
                                             continue;
                                         }

                                         let len = bytes.len() as u32;
                                         if let Err(e) = stream.write_all(&len.to_be_bytes()).await {
                                             warn!("Failed to write length to {}: {}", peer_addr, e);
                                             break;
                                         }

                                         if let Err(e) = stream.write_all(&bytes).await {
                                             warn!("Failed to write payload to {}: {}", peer_addr, e);
                                             break;
                                         }
                                    }
                                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                        warn!("Peer task {} lagged, skipped {} messages", peer_addr, skipped);
                                        continue;
                                    }
                                    Err(broadcast::error::RecvError::Closed) => {
                                        return; // System shutdown
                                    }
                                }
                            }
                            _ = shutdown_rx_peer.recv() => return,
                        }
                    }

                    if let Ok(mut map) = peer_map_clone.lock() {
                        map.remove(&peer_addr);
                    }
                    warn!("Connection to {} lost. Reconnecting...", peer_addr);

                    tokio::select! {
                        _ = tokio::time::sleep(retry_interval) => {},
                        _ = shutdown_rx_peer.recv() => return,
                    }
                }
            });
        }

        // 3. Listener Task (Incoming Connections)
        let bind_addr = config.bind_addr;
        let node_tx_clone = node_tx.clone();
        let mut shutdown_rx_listener = shutdown_rx.resubscribe();
        let local_height = config.local_height;
        let local_genesis_hash = config.local_genesis_hash;
        let local_protocol_version = config.local_protocol_version;
        let limits = ConnectionLimits {
            max_message_bytes: config.max_message_bytes,
            max_tx_bytes: config.max_tx_bytes,
            max_block_bytes: config.max_block_bytes,
            max_evidence_bytes: config.max_evidence_bytes,
            max_messages_per_sec: config.max_messages_per_sec,
            handshake_timeout: config.handshake_timeout,
            max_handshake_messages: config.max_handshake_messages,
        }
        .normalized();

        tokio::spawn(async move {
            let listener = match TcpListener::bind(bind_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind P2P port {}: {}", bind_addr, e);
                    return;
                }
            };
            let local_addr = match listener.local_addr() {
                Ok(addr) => addr,
                Err(e) => {
                    error!("Failed to get local address: {}", e);
                    return;
                }
            };
            info!("P2P Network listening on {}", local_addr);

            loop {
                tokio::select! {
                    res = listener.accept() => {
                        match res {
                            Ok((stream, addr)) => {
                                info!("Accepted connection from {}", addr);
                                let tx = node_tx_clone.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_incoming_connection(
                                        stream,
                                        tx,
                                        local_protocol_version,
                                        local_height,
                                        local_genesis_hash,
                                        limits,
                                    )
                                    .await
                                    {
                                        debug!("Connection closed with {}: {}", addr, e);
                                    }
                                });
                            }
                            Err(e) => error!("Accept failed: {}", e),
                        }
                    }
                    _ = shutdown_rx_listener.recv() => {
                        info!("P2P Listener shutting down...");
                        return;
                    }
                }
            }
        });

        (net_tx, node_rx, peer_map)
    }
}

async fn handle_incoming_connection(
    mut stream: TcpStream,
    tx: mpsc::Sender<NetworkMessage>,
    local_protocol_version: u64,
    local_height: u64,
    local_genesis_hash: StateHash,
    limits: ConnectionLimits,
) -> std::io::Result<()> {
    send_network_message(
        &mut stream,
        &NetworkMessage::StatusRequest,
        limits.max_message_bytes,
    )
    .await?;
    let mut verified = false;
    let handshake_started_at = std::time::Instant::now();
    let mut handshake_message_count: u32 = 0;
    let mut window_started_at = std::time::Instant::now();
    let mut window_count: u32 = 0;

    loop {
        if window_started_at.elapsed() >= Duration::from_secs(1) {
            window_started_at = std::time::Instant::now();
            window_count = 0;
        }
        window_count = window_count.saturating_add(1);
        if window_count > limits.max_messages_per_sec {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Peer message rate limit exceeded",
            ));
        }

        if !verified && handshake_started_at.elapsed() > limits.handshake_timeout {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Handshake timeout",
            ));
        }

        // 1. Read Length (4 bytes)
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > limits.max_message_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Message too large",
            ));
        }

        // 2. Read Payload
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;

        // 3. Deserialize
        match rmp_serde::from_slice::<NetworkMessage>(&buf) {
            Ok(msg) => match msg {
                NetworkMessage::StatusRequest => {
                    let resp = NetworkMessage::StatusResponse {
                        protocol_version: local_protocol_version,
                        height: local_height,
                        genesis_hash: local_genesis_hash,
                    };
                    send_network_message(&mut stream, &resp, limits.max_message_bytes).await?;
                }
                NetworkMessage::StatusResponse {
                    protocol_version,
                    genesis_hash,
                    ..
                } => {
                    if protocol_version != local_protocol_version {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Peer protocol version mismatch",
                        ));
                    }
                    if genesis_hash != local_genesis_hash {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Peer genesis mismatch",
                        ));
                    }
                    verified = true;
                }
                other => {
                    if !verified {
                        validate_message_payload_size(
                            &other,
                            limits.max_tx_bytes,
                            limits.max_block_bytes,
                            limits.max_evidence_bytes,
                        )?;
                        handshake_message_count = handshake_message_count.saturating_add(1);
                        if handshake_message_count > limits.max_handshake_messages {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::PermissionDenied,
                                "Too many messages before handshake completes",
                            ));
                        }
                        debug!("Dropping message before handshake completes: {:?}", other);
                        continue;
                    }
                    validate_message_payload_size(
                        &other,
                        limits.max_tx_bytes,
                        limits.max_block_bytes,
                        limits.max_evidence_bytes,
                    )?;
                    if let Err(e) = tx.send(other).await {
                        error!("Failed to forward message to node: {}", e);
                        return Ok(());
                    }
                }
            },
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }
    }
}

async fn perform_handshake(
    stream: &mut TcpStream,
    local_protocol_version: u64,
    local_height: u64,
    local_genesis_hash: StateHash,
    limits: ConnectionLimits,
) -> std::io::Result<()> {
    send_network_message(
        stream,
        &NetworkMessage::StatusRequest,
        limits.max_message_bytes,
    )
    .await?;

    let mut remaining = limits.handshake_timeout;
    let mut verified = false;

    while !verified {
        let start = std::time::Instant::now();
        let msg = tokio::time::timeout(
            remaining,
            read_network_message(stream, limits.max_message_bytes),
        )
        .await??;
        let elapsed = start.elapsed();
        remaining = remaining.saturating_sub(elapsed);

        match msg {
            NetworkMessage::StatusRequest => {
                let resp = NetworkMessage::StatusResponse {
                    protocol_version: local_protocol_version,
                    height: local_height,
                    genesis_hash: local_genesis_hash,
                };
                send_network_message(stream, &resp, limits.max_message_bytes).await?;
            }
            NetworkMessage::StatusResponse {
                protocol_version,
                genesis_hash,
                ..
            } => {
                if protocol_version != local_protocol_version {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Peer protocol version mismatch",
                    ));
                }
                if genesis_hash != local_genesis_hash {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Peer genesis mismatch",
                    ));
                }
                verified = true;
            }
            other => {
                debug!("Ignoring message during handshake: {:?}", other);
            }
        }
    }

    Ok(())
}

async fn send_network_message(
    stream: &mut TcpStream,
    msg: &NetworkMessage,
    max_message_bytes: usize,
) -> std::io::Result<()> {
    let bytes = rmp_serde::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if bytes.len() > max_message_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Outbound message too large",
        ));
    }
    let len = bytes.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

async fn read_network_message(
    stream: &mut TcpStream,
    max_message_bytes: usize,
) -> std::io::Result<NetworkMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > max_message_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    rmp_serde::from_slice::<NetworkMessage>(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn validate_message_payload_size(
    msg: &NetworkMessage,
    max_tx_bytes: usize,
    max_block_bytes: usize,
    max_evidence_bytes: usize,
) -> std::io::Result<()> {
    match msg {
        NetworkMessage::TransactionGossip(tx) => {
            let sz = rmp_serde::to_vec(tx)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .len();
            if sz > max_tx_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Transaction payload too large",
                ));
            }
        }
        NetworkMessage::BlockProposal(block) => {
            let sz = rmp_serde::to_vec(block)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .len();
            if sz > max_block_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Block payload too large",
                ));
            }
        }
        NetworkMessage::Proposal(p) => {
            let sz = rmp_serde::to_vec(&p.block)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .len();
            if sz > max_block_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Proposal block payload too large",
                ));
            }
        }
        NetworkMessage::Evidence(e) => {
            let sz = rmp_serde::to_vec(e)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .len();
            if sz > max_evidence_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Evidence payload too large",
                ));
            }
        }
        _ => {}
    }

    Ok(())
}
