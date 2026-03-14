//! Midnight block pull stage.
//!
//! This gasket stage connects to a Midnight node's Substrate JSON-RPC endpoint,
//! polls for newly finalised blocks, and emits [`PullEvent::RollForward`] events
//! downstream for each new block.
//!
//! The stage uses HTTP JSON-RPC polling (every [`POLL_INTERVAL_SECS`] seconds)
//! rather than a WebSocket subscription, which keeps the implementation simple
//! and avoids additional crate dependencies.

use gasket::framework::*;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::prelude::*;

/// JSON-decoded representation of a `chain_getBlock` response.
#[derive(serde::Deserialize, Debug)]
struct RpcBlock {
    header: RpcHeader,
    extrinsics: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RpcHeader {
    parent_hash: String,
    number: String,
    state_root: String,
    extrinsics_root: String,
}

const POLL_INTERVAL_SECS: u64 = 6;

pub type DownstreamPort = gasket::messaging::OutputPort<PullEvent>;

/// Work units for the midnight pull stage.
pub enum WorkUnit {
    Poll,
}

/// Gasket worker — holds the HTTP client and tracks the last seen block.
pub struct Worker {
    client: reqwest::Client,
}

impl Worker {
    /// Post a JSON-RPC 2.0 request and return the `result` field.
    async fn rpc_call(
        &self,
        url: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, WorkerError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let response = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| {
                warn!(error = %e, %method, "HTTP request to Midnight RPC failed");
                WorkerError::Retry
            })?;

        let text = response.text().await.map_err(|e| {
            warn!(error = %e, %method, "failed to read Midnight RPC response body");
            WorkerError::Retry
        })?;

        let resp: Value = serde_json::from_str(&text).map_err(|e| {
            warn!(error = %e, %method, body = %text, "failed to parse Midnight RPC response");
            WorkerError::Panic
        })?;

        if let Some(err) = resp.get("error") {
            warn!(?err, "JSON-RPC error from upstream");
            return Err(WorkerError::Retry);
        }

        Ok(resp["result"].clone())
    }

    /// Fetch the current finalised block hash.
    async fn get_finalized_head(&self, url: &str) -> Result<String, WorkerError> {
        let result = self
            .rpc_call(url, "chain_getFinalizedHead", serde_json::json!([]))
            .await?;

        result
            .as_str()
            .map(|s| s.to_owned())
            .ok_or(WorkerError::Panic)
    }

    /// Fetch the full block for the given hash.
    ///
    /// Returns `None` if the block is not found.
    async fn get_block(&self, url: &str, hash: &str) -> Result<Option<RpcBlock>, WorkerError> {
        let result = self
            .rpc_call(url, "chain_getBlock", serde_json::json!([hash]))
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        let block: RpcBlock = serde_json::from_value(result["block"].clone()).map_err(|e| {
            warn!(error = %e, "failed to deserialize chain_getBlock response");
            WorkerError::Panic
        })?;

        Ok(Some(block))
    }
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(_stage: &Stage) -> Result<Self, WorkerError> {
        Ok(Self {
            client: reqwest::Client::new(),
        })
    }

    async fn schedule(
        &mut self,
        _stage: &mut Stage,
    ) -> Result<WorkSchedule<WorkUnit>, WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
        Ok(WorkSchedule::Unit(WorkUnit::Poll))
    }

    async fn execute(&mut self, unit: &WorkUnit, stage: &mut Stage) -> Result<(), WorkerError> {
        match unit {
            WorkUnit::Poll => {
                debug!(url = %stage.rpc_url, "polling midnight RPC for finalized head");
                let hash = self.get_finalized_head(&stage.rpc_url).await?;

                // Skip if we've already processed this block.
                if stage.last_seen_hash.as_deref() == Some(hash.as_str()) {
                    debug!(%hash, "no new finalized block");
                    return Ok(());
                }

                let Some(block) = self.get_block(&stage.rpc_url, &hash).await? else {
                    warn!(%hash, "could not fetch block — skipping");
                    return Ok(());
                };

                let raw = dolos_midnight::encode_block(&hash, &block.header.number, &block.extrinsics)
                    .map_err(|e| {
                        warn!(error = %e, %hash, "failed to encode block");
                        WorkerError::Panic
                    })?;

                info!(
                    number = %block.header.number,
                    %hash,
                    extrinsics = block.extrinsics.len(),
                    "new finalized midnight block"
                );

                stage
                    .downstream
                    .send(PullEvent::RollForward(raw).into())
                    .await
                    .or_panic()?;

                stage.last_seen_hash = Some(hash);
                stage.block_count.inc(1);

                Ok(())
            }
        }
    }
}

/// Gasket stage — polls the Substrate JSON-RPC endpoint for finalised blocks.
#[derive(Stage)]
#[stage(name = "midnight_pull", unit = "WorkUnit", worker = "Worker")]
pub struct Stage {
    /// URL of the Substrate JSON-RPC HTTP endpoint (e.g. `http://localhost:9944`).
    pub rpc_url: String,

    /// Hash of the last block we emitted downstream.
    last_seen_hash: Option<String>,

    pub downstream: DownstreamPort,

    #[metric]
    block_count: gasket::metrics::Counter,
}

impl Stage {
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url,
            last_seen_hash: None,
            downstream: Default::default(),
            block_count: Default::default(),
        }
    }
}
