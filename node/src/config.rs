use clap::Parser;
use config::{Config, ConfigError, Environment, File};
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    pub node_id: String,
    pub data_dir: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub listen_address: String,
    #[serde(default, deserialize_with = "deserialize_peers")]
    pub peers: Option<Vec<String>>,
    #[serde(default)]
    pub peer_api_map: Option<std::collections::HashMap<String, String>>,
}

fn deserialize_peers<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct PeersVisitor;

    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    value.split(',').map(|s| s.trim().to_string()).collect(),
                ))
            }
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element()? {
                vec.push(elem);
            }
            Ok(Some(vec))
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(PeersVisitor)
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    pub enabled: bool,
    pub bind_address: String,
    #[serde(default)]
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub sqlite_path: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct GenesisConfig {
    pub genesis_file: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct MempoolConfig {
    pub max_size: u64,
    pub max_tx_bytes: u64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConsoleConfig {
    pub user: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub api: ApiConfig,
    pub storage: StorageConfig,
    pub genesis: GenesisConfig,
    pub mempool: MempoolConfig,
    pub logging: LoggingConfig,
    pub console: ConsoleConfig,
    pub validator: ValidatorConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ValidatorConfig {
    /// Validator private key (hex-encoded). Skipped during config file deserialization.
    /// Set programmatically for tests, or via AXIOM_VALIDATOR_PRIVATE_KEY env var at runtime.
    #[serde(skip)]
    pub private_key: Option<String>,
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.node.node_id.is_empty() {
            return Err(ConfigError::Message("node_id cannot be empty".to_string()));
        }
        if self.network.enabled && self.network.listen_address.is_empty() {
            return Err(ConfigError::Message(
                "network.listen_address cannot be empty if network is enabled".to_string(),
            ));
        }
        if self.api.enabled && self.api.bind_address.is_empty() {
            return Err(ConfigError::Message(
                "api.bind_address cannot be empty if api is enabled".to_string(),
            ));
        }
        if !self.genesis.genesis_file.exists() {
            return Err(ConfigError::Message(format!(
                "genesis_file does not exist: {:?}",
                self.genesis.genesis_file
            )));
        }
        if self.mempool.max_size == 0 {
            return Err(ConfigError::Message("mempool.max_size must be > 0".to_string()));
        }
        if self.mempool.max_tx_bytes == 0 {
            return Err(ConfigError::Message(
                "mempool.max_tx_bytes must be > 0".to_string(),
            ));
        }
        if self.mempool.max_size > (usize::MAX as u64) {
            return Err(ConfigError::Message(
                "mempool.max_size exceeds platform limits".to_string(),
            ));
        }
        if self.mempool.max_tx_bytes > (usize::MAX as u64) {
            return Err(ConfigError::Message(
                "mempool.max_tx_bytes exceeds platform limits".to_string(),
            ));
        }
        if self.logging.level.is_empty() {
            return Err(ConfigError::Message("logging.level cannot be empty".to_string()));
        }
        if self.logging.format.to_lowercase() != "json" {
            return Err(ConfigError::Message(
                "logging.format must be 'json'".to_string(),
            ));
        }
        if self.api.enabled && self.console.user.is_empty() {
            return Err(ConfigError::Message("console.user cannot be empty".to_string()));
        }
        if self.api.enabled && self.console.password.is_empty() {
            return Err(ConfigError::Message(
                "console.password cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Path to configuration file
    #[arg(long, default_value = "axiom.toml")]
    pub config: PathBuf,

    // Node overrides
    #[arg(long)]
    pub node_id: Option<String>,
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    // Network overrides
    #[arg(long)]
    pub network_enabled: Option<bool>,
    #[arg(long)]
    pub network_listen_address: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub network_peers: Option<Vec<String>>,

    // API overrides
    #[arg(long)]
    pub api_enabled: Option<bool>,
    #[arg(long)]
    pub api_bind_address: Option<String>,
    #[arg(long)]
    pub api_tls_enabled: Option<bool>,
    #[arg(long)]
    pub api_tls_cert: Option<PathBuf>,
    #[arg(long)]
    pub api_tls_key: Option<PathBuf>,

    // Storage overrides
    #[arg(long)]
    pub storage_sqlite_path: Option<PathBuf>,

    // Genesis overrides
    #[arg(long)]
    pub genesis_file: Option<PathBuf>,

    // Logging overrides
    #[arg(long)]
    pub logging_level: Option<String>,
    #[arg(long)]
    pub logging_format: Option<String>,
    #[arg(long)]
    pub mempool_max_size: Option<u64>,
    #[arg(long)]
    pub mempool_max_tx_bytes: Option<u64>,
    #[arg(long)]
    pub console_user: Option<String>,
    #[arg(long)]
    pub console_password: Option<String>,
    // Validator private key loaded from AXIOM_VALIDATOR_PRIVATE_KEY env var (CODING_RULES 5.3)
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let args = CliArgs::parse();

        let builder = Config::builder()
            .add_source(File::from(args.config.clone()).required(true))
            .add_source(Environment::with_prefix("AXIOM").separator("__"));

        let mut builder = builder;

        if let Some(v) = args.node_id {
            builder = builder.set_override("node.node_id", v)?;
        }
        if let Some(v) = args.data_dir {
            let path_str = v.to_str().ok_or_else(|| {
                ConfigError::Message("data_dir path contains invalid UTF-8".to_string())
            })?;
            builder = builder.set_override("node.data_dir", path_str)?;
        }

        if let Some(v) = args.network_enabled {
            builder = builder.set_override("network.enabled", v)?;
        }
        if let Some(v) = args.network_listen_address {
            builder = builder.set_override("network.listen_address", v)?;
        }
        if let Some(v) = args.network_peers {
            builder = builder.set_override("network.peers", v)?;
        }

        if let Some(v) = args.api_enabled {
            builder = builder.set_override("api.enabled", v)?;
        }
        if let Some(v) = args.api_bind_address {
            builder = builder.set_override("api.bind_address", v)?;
        }
        if let Some(v) = args.api_tls_enabled {
            builder = builder.set_override("api.tls_enabled", v)?;
        }
        if let Some(v) = args.api_tls_cert {
            let path_str = v.to_str().ok_or_else(|| {
                ConfigError::Message("tls_cert_path contains invalid UTF-8".to_string())
            })?;
            builder = builder.set_override("api.tls_cert_path", path_str)?;
        }
        if let Some(v) = args.api_tls_key {
            let path_str = v.to_str().ok_or_else(|| {
                ConfigError::Message("tls_key_path contains invalid UTF-8".to_string())
            })?;
            builder = builder.set_override("api.tls_key_path", path_str)?;
        }

        if let Some(v) = args.storage_sqlite_path {
            let path_str = v.to_str().ok_or_else(|| {
                ConfigError::Message("sqlite_path contains invalid UTF-8".to_string())
            })?;
            builder = builder.set_override("storage.sqlite_path", path_str)?;
        }

        if let Some(v) = args.genesis_file {
            let path_str = v.to_str().ok_or_else(|| {
                ConfigError::Message("genesis_file path contains invalid UTF-8".to_string())
            })?;
            builder = builder.set_override("genesis.genesis_file", path_str)?;
        }

        if let Some(v) = args.logging_level {
            builder = builder.set_override("logging.level", v)?;
        }

        // Validator private key is read from AXIOM_VALIDATOR_PRIVATE_KEY env var only (CODING_RULES 5.3)
        if let Some(v) = args.logging_format {
            builder = builder.set_override("logging.format", v)?;
        }

        if let Some(v) = args.mempool_max_size {
            builder = builder.set_override("mempool.max_size", v)?;
        }
        if let Some(v) = args.mempool_max_tx_bytes {
            builder = builder.set_override("mempool.max_tx_bytes", v)?;
        }
        if let Some(v) = args.console_user {
            builder = builder.set_override("console.user", v)?;
        }
        if let Some(v) = args.console_password {
            builder = builder.set_override("console.password", v)?;
        }

        builder.build()?.try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_structure() {
        // We can't easily test file loading here without a file, but we can test that
        // the structs deserialize correctly from a config source.

        let config_str = r#"
            [node]
            node_id = "test-node"
            data_dir = "./test-data"

            [network]
            enabled = true
            listen_address = "127.0.0.1:7000"
            peers = ["1.1.1.1:7000"]

            [api]
            enabled = true
            bind_address = "127.0.0.1:8000"

            [storage]
            sqlite_path = "./test.db"

            [genesis]
            genesis_file = "./genesis.json"

            [mempool]
            max_size = 10000
            max_tx_bytes = 65536

            [logging]
            level = "debug"
            format = "json"

            [console]
            user = "operator"
            password = "axiom"

            [validator]
        "#;

        let c = Config::builder()
            .add_source(File::from_str(config_str, config::FileFormat::Toml))
            .build()
            .unwrap();

        let app_config: AppConfig = c.try_deserialize().unwrap();

        assert_eq!(app_config.node.node_id, "test-node");
        assert_eq!(app_config.network.peers.unwrap()[0], "1.1.1.1:7000");
    }

    #[test]
    fn test_environment_override() {
        let config_str = r#"
            [node]
            node_id = "test-node"
            data_dir = "./test-data"
            [network]
            enabled = true
            listen_address = "127.0.0.1:7000"
            [api]
            enabled = true
            bind_address = "127.0.0.1:8000"
            [storage]
            sqlite_path = "./test.db"
            [genesis]
            genesis_file = "./genesis.json"
            [mempool]
            max_size = 10000
            max_tx_bytes = 65536
            [logging]
            level = "info"
            format = "json"
            [console]
            user = "operator"
            password = "axiom"
            [validator]
            # No private key by default
        "#;

        // Try with double underscore after prefix too, just in case
        temp_env::with_var("AXIOM__NODE__NODE_ID", Some("env-node"), || {
            let c = Config::builder()
                .add_source(File::from_str(config_str, config::FileFormat::Toml))
                .add_source(Environment::with_prefix("AXIOM").separator("__"))
                .build()
                .unwrap();

            let app_config: AppConfig = c.try_deserialize().unwrap();
            assert_eq!(app_config.node.node_id, "env-node");
        });
    }
}
