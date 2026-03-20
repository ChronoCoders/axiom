use axiom_execution::compute_state_hash;
use axiom_primitives::PROTOCOL_VERSION;
use tokio::signal;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;

use axiom_node::config::AppConfig;
use axiom_node::genesis::load_genesis_state;
use axiom_node::node;

// LOCKED GENESIS HASH
const LOCKED_GENESIS_HASH: &str =
    "c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761";

// Helper for shutdown signal
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            eprintln!("Failed to install Ctrl+C handler: {e}");
            std::process::exit(1);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        let mut sig =
            signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap_or_else(|e| {
                eprintln!("Failed to install signal handler: {e}");
                std::process::exit(1);
            });
        sig.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main]
async fn main() {
    // 1. Load Configuration
    let config = match AppConfig::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = config.validate() {
        eprintln!("Invalid configuration: {e}");
        std::process::exit(1);
    }

    // 2. Initialize Logging
    let level = match config.logging.level.to_lowercase().as_str() {
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "info" => LevelFilter::INFO,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        other => {
            eprintln!("Invalid logging.level: {other}");
            std::process::exit(1);
        }
    };

    if config.logging.format.to_lowercase() != "json" {
        eprintln!("Invalid logging.format: {} (must be 'json')", config.logging.format);
        std::process::exit(1);
    }

    let subscriber = FmtSubscriber::builder().with_max_level(level).json().finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("Failed to set global default subscriber");
        std::process::exit(1);
    }

    tracing::info!("Starting AXIOM Node (Binary): {}", config.node.node_id);
    tracing::info!("Protocol Version: {}", PROTOCOL_VERSION);

    // GENESIS HASH CHECK
    match load_genesis_state(&config.genesis.genesis_file) {
        Ok(genesis_state) => {
            let hash = compute_state_hash(&genesis_state);
            tracing::info!("Verifying Genesis Hash: {}", hex::encode(hash.0));

            // Verify strict genesis hash
            if hash.0.as_slice()
                != hex::decode(LOCKED_GENESIS_HASH)
                    .unwrap_or_default()
                    .as_slice()
            {
                tracing::error!("CRITICAL: Genesis hash mismatch!");
                tracing::error!("Expected: {}", LOCKED_GENESIS_HASH);
                tracing::error!("Computed: {}", hex::encode(hash.0));
                std::process::exit(1);
            }
        }
        Err(e) => {
            tracing::error!("CRITICAL: Could not load genesis for verification: {}", e);
            std::process::exit(1);
        }
    }

    // Shutdown Signal Channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    // Spawn signal handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("Shutdown signal received, starting graceful shutdown...");
        let _ = shutdown_tx_clone.send(());
    });

    // Start Node
    node::start(config, shutdown_rx).await;
}
