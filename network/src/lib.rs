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
                                    if let Err(e) = handle_incoming_connection(stream, tx).await {
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
) -> std::io::Result<()> {
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
                if let Err(e) = tx.send(msg).await {
                    error!("Failed to forward message to node: {}", e);
                    return Ok(());
                }
            }
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }
    }
}
