use axiom_crypto::test_keypair;
use axiom_node::config::{
    ApiConfig, AppConfig, ConsoleConfig, GenesisConfig, LoggingConfig, MempoolConfig, NetworkConfig,
    NodeConfig, StorageConfig,
};
use axiom_node::node;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::broadcast;

// Helper to setup a test environment
fn setup_test_env(id: &str) -> (String, String) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base_dir = format!("test_data_{id}_{timestamp}");
    // Clean up if exists (shouldn't happen with timestamp)
    if std::path::Path::new(&base_dir).exists() {
        fs::remove_dir_all(&base_dir).expect("Failed to remove existing test directory");
    }
    fs::create_dir_all(&base_dir).unwrap();

    let genesis_path = format!("{base_dir}/genesis.json");

    (base_dir, genesis_path)
}

fn write_valid_genesis(path: &str, validators: Vec<String>) {
    let mut accounts_json = String::new();
    let mut validators_json = String::new();

    for (i, id) in validators.iter().enumerate() {
        if i > 0 {
            accounts_json.push_str(",\n");
            validators_json.push_str(",\n");
        }
        accounts_json.push_str(&format!(
            r#"    {{
      "id": "{id}",
      "balance": 250000,
      "nonce": 0
    }}"#
        ));

        validators_json.push_str(&format!(
            r#"    {{
      "id": "{id}",
      "voting_power": 10,
      "account_id": "{id}",
      "active": true
    }}"#
        ));
    }

    let content = format!(
        r#"{{
  "height": 0,
  "epoch": 0,
  "total_supply": {},
  "block_reward": 10,
  "accounts": [
{}
  ],
  "validators": [
{}
  ]
}}"#,
        250000 * validators.len() as u64,
        accounts_json,
        validators_json
    );

    fs::write(path, content).unwrap();
}

fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct TestNode {
    shutdown_tx: broadcast::Sender<()>,
    pub api_port: u16,
    pub _p2p_port: u16,
    pub node_id: String,
    pub tls: bool,
    _base_dir: String,
    _handle: tokio::task::JoinHandle<()>,

    // For restart
    _private_key: String,
    _genesis_path: String,
}

impl Drop for TestNode {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        // Attempt to clean up test directory
        // Note: This might fail if files are locked by the process, but we try.
        // For tests, it's often better to rely on OS temp dirs or gitignore.
        // Here we rely on gitignore "test_data_*"
    }
}

impl TestNode {
    async fn new(
        id: &str,
        peers: Vec<String>,
        validator_key: Option<&str>,
        tls_config: Option<(&str, &str)>,
        genesis_validators: Option<Vec<String>>,
        fixed_ports: Option<(u16, u16)>,
    ) -> Self {
        let (base_dir, genesis_path) = setup_test_env(id);

        // Use provided key or generate one
        let private_key = if let Some(k) = validator_key {
            k.to_string()
        } else {
            let (pk, _) = test_keypair("validator1");
            hex::encode(pk.to_bytes())
        };

        let validators = genesis_validators.unwrap_or_else(|| {
            let (_, pk) = test_keypair("validator1");
            vec![hex::encode(pk.0)]
        });

        write_valid_genesis(&genesis_path, validators);

        let (tls_cert, tls_key) = if let Some((c, k)) = tls_config {
            (Some(PathBuf::from(c)), Some(PathBuf::from(k)))
        } else {
            (None, None)
        };

        let (api_port, p2p_port) =
            fixed_ports.unwrap_or_else(|| (get_free_port(), get_free_port()));

        let config = AppConfig {
            node: NodeConfig {
                node_id: id.to_string(),
                data_dir: PathBuf::from(&base_dir),
            },
            network: NetworkConfig {
                enabled: true,
                listen_address: format!("127.0.0.1:{p2p_port}"),
                peers: Some(peers),
                peer_api_map: None,
            },
            api: ApiConfig {
                enabled: true,
                bind_address: format!("127.0.0.1:{api_port}"),
                tls_enabled: tls_config.is_some(),
                tls_cert_path: tls_cert,
                tls_key_path: tls_key,
            },
            storage: StorageConfig {
                sqlite_path: PathBuf::from(format!("{base_dir}/axiom.db")),
            },
            genesis: GenesisConfig {
                genesis_file: PathBuf::from(&genesis_path),
            },
            mempool: MempoolConfig {
                max_size: 10000,
                max_tx_bytes: 65536,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "json".to_string(),
            },
            console: ConsoleConfig {
                user: "operator".to_string(),
                password: "axiom".to_string(),
            },
            validator: axiom_node::config::ValidatorConfig {
                private_key: Some(private_key.clone()),
            },
        };

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let handle = tokio::spawn(async move {
            node::start(config, shutdown_rx).await;
        });

        TestNode {
            shutdown_tx,
            api_port,
            _p2p_port: p2p_port,
            node_id: id.to_string(),
            tls: tls_config.is_some(),
            _base_dir: base_dir,
            _handle: handle,
            _private_key: private_key,
            _genesis_path: genesis_path,
        }
    }

    async fn wait_for_ready(&self) {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            // Simple TCP connect check first to ensure port is open
            if std::net::TcpStream::connect(format!("127.0.0.1:{}", self.api_port)).is_ok() {
                let client = reqwest::Client::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .unwrap();

                let protocol = if self.tls { "https" } else { "http" };
                let url = format!("{}://127.0.0.1:{}/health/live", protocol, self.api_port);

                if let Ok(resp) = client.get(&url).send().await {
                    if resp.status().is_success() {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("Node {} failed to become ready", self.node_id);
    }
}

#[tokio::test]
async fn test_genesis_load_success() {
    let node = TestNode::new("success", vec![], None, None, None, None).await;
    node.wait_for_ready().await;
    // Node is running and healthy
}

#[tokio::test]
async fn test_tls_api() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let id = "tls_test";
    let (base_dir, _) = setup_test_env(id);

    // Generate Certs
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let cert_path = format!("{base_dir}/cert.pem");
    let key_path = format!("{base_dir}/key.pem");

    fs::write(&cert_path, &cert_pem).unwrap();
    fs::write(&key_path, &key_pem).unwrap();

    let node = TestNode::new(id, vec![], None, Some((&cert_path, &key_path)), None, None).await;
    node.wait_for_ready().await;

    // Make HTTPS request
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let resp = client
        .get(format!("https://localhost:{}/api/status", node.api_port))
        .send()
        .await;

    match resp {
        Ok(r) => assert_eq!(r.status(), 200),
        Err(e) => panic!("HTTPS request failed: {e}"),
    }
}
