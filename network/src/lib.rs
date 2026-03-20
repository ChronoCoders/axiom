use axiom_primitives::Block;
use axiom_primitives::BlockHash;
use axiom_primitives::StateHash;
use axiom_primitives::Transaction;
use axiom_primitives::ValidatorSignature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NetworkMessage {
    BlockProposal(Block),
    Vote(ValidatorSignature, BlockHash, u64), // sig, block_hash, height
    TransactionGossip(Transaction),
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

                    if let Err(e) =
                        perform_handshake(
                            &mut stream,
                            local_protocol_version,
                            local_height,
                            local_genesis_hash,
                        )
                        .await
                    {
                        warn!("Handshake failed with {}: {}. Reconnecting...", peer_addr, e);
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
                                         let bytes = match bincode::serialize(&msg) {
                                            Ok(b) => b,
                                            Err(e) => {
                                                error!("Serialization error: {}", e);
                                                continue;
                                            }
                                         };

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
) -> std::io::Result<()> {
    send_network_message(&mut stream, &NetworkMessage::StatusRequest).await?;
    let mut verified = false;

    loop {
        // 1. Read Length (4 bytes)
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > 10 * 1024 * 1024 {
            // 10MB Safety limit
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Message too large",
            ));
        }

        // 2. Read Payload
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;

        // 3. Deserialize
        match bincode::deserialize::<NetworkMessage>(&buf) {
            Ok(msg) => {
                match msg {
                    NetworkMessage::StatusRequest => {
                        let resp = NetworkMessage::StatusResponse {
                            protocol_version: local_protocol_version,
                            height: local_height,
                            genesis_hash: local_genesis_hash,
                        };
                        send_network_message(&mut stream, &resp).await?;
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
                            debug!("Dropping message before handshake completes: {:?}", other);
                            continue;
                        }
                        if let Err(e) = tx.send(other).await {
                            error!("Failed to forward message to node: {}", e);
                            return Ok(());
                        }
                    }
                }
            }
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
) -> std::io::Result<()> {
    send_network_message(stream, &NetworkMessage::StatusRequest).await?;

    let mut remaining = Duration::from_secs(3);
    let mut verified = false;

    while !verified {
        let start = std::time::Instant::now();
        let msg = tokio::time::timeout(remaining, read_network_message(stream)).await??;
        let elapsed = start.elapsed();
        remaining = remaining.saturating_sub(elapsed);

        match msg {
            NetworkMessage::StatusRequest => {
                let resp = NetworkMessage::StatusResponse {
                    protocol_version: local_protocol_version,
                    height: local_height,
                    genesis_hash: local_genesis_hash,
                };
                send_network_message(stream, &resp).await?;
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

async fn send_network_message(stream: &mut TcpStream, msg: &NetworkMessage) -> std::io::Result<()> {
    let bytes = bincode::serialize(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = bytes.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

async fn read_network_message(stream: &mut TcpStream) -> std::io::Result<NetworkMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 10 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    bincode::deserialize::<NetworkMessage>(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
