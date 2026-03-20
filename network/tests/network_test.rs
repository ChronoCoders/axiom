use axiom_network::{Network, NetworkConfig, NetworkMessage};
use axiom_primitives::{
    AccountId, Block, BlockHash, Signature, StateHash, Transaction, TransactionType, ValidatorId,
    ValidatorSignature,
};
use std::net::SocketAddr;
use tokio::time::{sleep, timeout, Duration};

// Helper to create dummy data
fn dummy_block() -> Block {
    Block {
        parent_hash: BlockHash([0; 32]),
        height: 1,
        epoch: 1,
        proposer_id: ValidatorId([1; 32]),
        transactions: vec![],
        signatures: vec![],
        state_hash: StateHash([2; 32]),
        timestamp: 0,
    }
}

fn dummy_tx() -> Transaction {
    Transaction {
        sender: AccountId([3; 32]),
        recipient: AccountId([4; 32]),
        amount: 100,
        nonce: 1,
        signature: Signature([5; 64]),
        tx_type: TransactionType::Transfer,
    }
}

fn dummy_vote() -> NetworkMessage {
    NetworkMessage::Vote(
        ValidatorSignature {
            validator_id: ValidatorId([6; 32]),
            signature: Signature([7; 64]),
        },
        BlockHash([8; 32]),
        10,
    )
}

async fn get_free_port() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap()
}

#[tokio::test]
async fn test_serialization_variants() {
    let variants = vec![
        NetworkMessage::BlockProposal(dummy_block()),
        dummy_vote(),
        NetworkMessage::TransactionGossip(dummy_tx()),
        NetworkMessage::StatusRequest,
        NetworkMessage::StatusResponse {
            protocol_version: 1,
            height: 5,
            genesis_hash: StateHash([9; 32]),
        },
    ];

    for msg in variants {
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: NetworkMessage = bincode::deserialize(&encoded).unwrap();
        // Just checking it doesn't panic and is same variant
        match (msg, decoded) {
            (NetworkMessage::BlockProposal(_), NetworkMessage::BlockProposal(_)) => {}
            (NetworkMessage::Vote(..), NetworkMessage::Vote(..)) => {}
            (NetworkMessage::TransactionGossip(_), NetworkMessage::TransactionGossip(_)) => {}
            (NetworkMessage::StatusRequest, NetworkMessage::StatusRequest) => {}
            (NetworkMessage::StatusResponse { .. }, NetworkMessage::StatusResponse { .. }) => {}
            _ => panic!("Variant mismatch after serialization"),
        }
    }
}

use tokio::sync::broadcast;

#[tokio::test]
async fn test_peer_unavailable() {
    let addr_a = get_free_port().await;
    // Point to a port that is definitely closed (we just bound it then dropped it in get_free_port, but let's be sure)
    // Actually get_free_port returns a port that WAS free. If we don't bind it, it's closed.
    // But Network::start binds it.
    // We want a peer that is NOT running.
    let closed_peer_addr: SocketAddr = "127.0.0.1:54321".parse().unwrap();

    let config = NetworkConfig {
        bind_addr: addr_a,
        peers: vec![closed_peer_addr],
        retry_interval: Some(Duration::from_millis(100)),
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: StateHash([9; 32]),
        local_protocol_version: 1,
    };

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (tx, _rx, _peers) = Network::start(config, shutdown_rx).await;

    // Sending should not crash even if peer is down
    let msg = NetworkMessage::StatusRequest;
    // We send and wait a bit. The log should show a warning, but no panic.
    tx.send(msg).await.unwrap();
    sleep(Duration::from_millis(100)).await;
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_peer_reconnection() {
    let addr_a = get_free_port().await;
    let addr_b = get_free_port().await;
    let genesis_hash = StateHash([9; 32]);

    // Config for Node A: connects to B, with retry
    let config_a = NetworkConfig {
        bind_addr: addr_a,
        peers: vec![addr_b],
        retry_interval: Some(Duration::from_millis(100)),
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: genesis_hash,
        local_protocol_version: 1,
    };

    // Config for Node B: listens
    let config_b = NetworkConfig {
        bind_addr: addr_b,
        peers: vec![],
        retry_interval: None,
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: genesis_hash,
        local_protocol_version: 1,
    };

    // Start Node A
    let (shutdown_tx_a, shutdown_rx_a) = broadcast::channel(1);
    let (tx_a, _rx_a, _peers_a) = Network::start(config_a, shutdown_rx_a).await;

    // Node B is not up yet. Node A should be retrying.
    // Wait a bit
    sleep(Duration::from_millis(300)).await;

    // Start Node B
    let (shutdown_tx_b, shutdown_rx_b) = broadcast::channel(1);
    let (_tx_b, mut rx_b, _peers_b) = Network::start(config_b, shutdown_rx_b).await;

    // They should connect. Node A should send handshake/messages.
    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Send message from A to B to verify connection
    tx_a.send(NetworkMessage::TransactionGossip(dummy_tx()))
        .await
        .unwrap();

    // Check if B received it
    match timeout(Duration::from_secs(2), rx_b.recv()).await {
        Ok(Some(NetworkMessage::TransactionGossip(_))) => {}
        Ok(x) => panic!("Expected TransactionGossip, got {x:?}"),
        Err(_) => panic!("Timed out waiting for message"),
    }

    let _ = shutdown_tx_a.send(());
    let _ = shutdown_tx_b.send(());
}

#[tokio::test]
async fn test_3_node_communication() {
    let addr_a = get_free_port().await;
    let addr_b = get_free_port().await;
    let addr_c = get_free_port().await;
    let genesis_hash = StateHash([9; 32]);

    // A knows B, B knows C, C knows A (Ring)
    let config_a = NetworkConfig {
        bind_addr: addr_a,
        peers: vec![addr_b],
        retry_interval: Some(Duration::from_millis(100)),
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: genesis_hash,
        local_protocol_version: 1,
    };
    let config_b = NetworkConfig {
        bind_addr: addr_b,
        peers: vec![addr_c],
        retry_interval: Some(Duration::from_millis(100)),
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: genesis_hash,
        local_protocol_version: 1,
    };
    let config_c = NetworkConfig {
        bind_addr: addr_c,
        peers: vec![addr_a],
        retry_interval: Some(Duration::from_millis(100)),
        peer_api_map: std::collections::HashMap::new(),
        local_height: 0,
        local_genesis_hash: genesis_hash,
        local_protocol_version: 1,
    };

    let (shutdown_tx_a, shutdown_rx_a) = broadcast::channel(1);
    let (shutdown_tx_b, shutdown_rx_b) = broadcast::channel(1);
    let (shutdown_tx_c, shutdown_rx_c) = broadcast::channel(1);

    let (tx_a, mut rx_a, _pa) = Network::start(config_a, shutdown_rx_a).await;
    let (tx_b, mut rx_b, _pb) = Network::start(config_b, shutdown_rx_b).await;
    let (tx_c, mut rx_c, _pc) = Network::start(config_c, shutdown_rx_c).await;

    // Wait for connections
    sleep(Duration::from_secs(2)).await;

    // A sends to B
    tx_a.send(NetworkMessage::TransactionGossip(dummy_tx()))
        .await
        .unwrap();

    // B receives
    match timeout(Duration::from_secs(2), rx_b.recv()).await {
        Ok(Some(NetworkMessage::TransactionGossip(_))) => {}
        Ok(x) => panic!("B expected TransactionGossip, got {x:?}"),
        Err(_) => panic!("B Timed out waiting for message"),
    }

    // B sends to C
    tx_b.send(NetworkMessage::TransactionGossip(dummy_tx()))
        .await
        .unwrap();

    // C receives
    match timeout(Duration::from_secs(2), rx_c.recv()).await {
        Ok(Some(NetworkMessage::TransactionGossip(_))) => {}
        Ok(x) => panic!("C expected TransactionGossip, got {x:?}"),
        Err(_) => panic!("C Timed out waiting for message"),
    }

    // C sends to A
    tx_c.send(NetworkMessage::TransactionGossip(dummy_tx()))
        .await
        .unwrap();

    // A receives
    match timeout(Duration::from_secs(2), rx_a.recv()).await {
        Ok(Some(NetworkMessage::TransactionGossip(_))) => {}
        Ok(x) => panic!("A expected TransactionGossip, got {x:?}"),
        Err(_) => panic!("A Timed out waiting for message"),
    }

    let _ = shutdown_tx_a.send(());
    let _ = shutdown_tx_b.send(());
    let _ = shutdown_tx_c.send(());
}
