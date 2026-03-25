use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightConfig {
    pub node: NodeConfig,
    pub storage: StorageConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub ws_url: String,
    /// Number of blocks to buffer before flushing as a batch.
    /// Higher values improve throughput during catch-up; during live sync
    /// blocks arrive slowly so batches will typically be smaller.
    #[serde(default = "default_batch_size")]
    pub sync_batch_size: usize,
}

fn default_batch_size() -> usize {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub listen_address: SocketAddr,
}

impl Default for MidnightConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig {
                ws_url: "ws://localhost:9944".to_string(),
                sync_batch_size: default_batch_size(),
            },
            storage: StorageConfig {
                path: PathBuf::from("./midnight-data"),
            },
            api: ApiConfig {
                listen_address: "0.0.0.0:3001".parse().unwrap(),
            },
        }
    }
}
