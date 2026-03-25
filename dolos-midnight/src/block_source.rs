use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Extracted transaction — midnight tx bytes pre-decoded from extrinsics
// ---------------------------------------------------------------------------

/// A midnight transaction extracted from a substrate extrinsic during sync.
///
/// By the time these reach the parser, the SCALE extrinsic envelope has already
/// been decoded (via `as_root_extrinsic::<Call>()` on the live connection).
/// Only the inner midnight payload bytes remain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMidnightTx {
    pub kind: TxKind,
    /// The raw midnight transaction bytes (payload of `send_mn_transaction`
    /// or `send_mn_system_transaction`).
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxKind {
    Regular,
    System,
}

// ---------------------------------------------------------------------------
// RawSubstrateBlock — the wire format between sync and chain logic
// ---------------------------------------------------------------------------

/// A raw Substrate block as received from the node via subxt.
///
/// This struct is serialized to bincode and stored as `RawBlock` bytes in the
/// WAL and archive. `MidnightLogic::receive_block()` deserializes it back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSubstrateBlock {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    /// Midnight transactions extracted from extrinsics during sync.
    /// Already decoded from the SCALE Call envelope — ready for
    /// `tagged_deserialize` by the parser.
    pub midnight_txs: Vec<RawMidnightTx>,
    /// Block timestamp from the Timestamp pallet (if present).
    pub timestamp: Option<u64>,
}

// ---------------------------------------------------------------------------
// BlockSource trait
// ---------------------------------------------------------------------------

/// Error type for block source operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("decode error: {0}")]
    Decode(String),

    #[error("subscription ended")]
    SubscriptionEnded,

    #[error("block not found: {0}")]
    BlockNotFound(u64),
}

/// Abstraction over the Midnight node connection.
#[async_trait::async_trait]
pub trait BlockSource: Send + Sync + 'static {
    /// Subscribe to finalized blocks starting from the given block number.
    /// If `from_block` is `None`, starts from the latest finalized block.
    async fn subscribe_finalized(
        &self,
        from_block: Option<u64>,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_core::Stream<Item = Result<RawSubstrateBlock, SyncError>>
                    + Send
                    + 'static,
            >,
        >,
        SyncError,
    >;

    /// Fetch a single block by number.
    async fn fetch_block(&self, number: u64) -> Result<RawSubstrateBlock, SyncError>;

    /// Fetch multiple blocks concurrently.
    ///
    /// Default implementation calls `fetch_block` sequentially. Implementations
    /// with a persistent connection (e.g. subxt over WebSocket) should override
    /// this to fire concurrent requests for better throughput.
    async fn fetch_blocks(
        &self,
        numbers: &[u64],
    ) -> Result<Vec<RawSubstrateBlock>, SyncError> {
        let mut blocks = Vec::with_capacity(numbers.len());
        for &n in numbers {
            blocks.push(self.fetch_block(n).await?);
        }
        Ok(blocks)
    }
}

// ---------------------------------------------------------------------------
// MockBlockSource — for testing / default
// ---------------------------------------------------------------------------

pub struct MockBlockSource;

#[async_trait::async_trait]
impl BlockSource for MockBlockSource {
    async fn subscribe_finalized(
        &self,
        _from_block: Option<u64>,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_core::Stream<Item = Result<RawSubstrateBlock, SyncError>>
                    + Send
                    + 'static,
            >,
        >,
        SyncError,
    > {
        Ok(Box::pin(futures_util::stream::empty()))
    }

    async fn fetch_block(&self, number: u64) -> Result<RawSubstrateBlock, SyncError> {
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&number.to_le_bytes());

        let mut parent_hash = [0u8; 32];
        if number > 0 {
            parent_hash[..8].copy_from_slice(&(number - 1).to_le_bytes());
        }

        Ok(RawSubstrateBlock {
            number,
            hash,
            parent_hash,
            midnight_txs: Vec::new(),
            timestamp: Some(number * 12_000),
        })
    }
}

// ---------------------------------------------------------------------------
// SubxtBlockSource — real WebSocket block source (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "subxt-sync")]
mod subxt_source {
    use std::pin::Pin;
    use std::time::Duration;

    use futures_util::{StreamExt, TryStreamExt};
    use subxt::backend::legacy::LegacyRpcMethods;
    use subxt::backend::rpc::reconnecting_rpc_client::{ExponentialBackoff, RpcClient};
    use subxt::blocks::Extrinsics;
    use subxt::{OnlineClient, SubstrateConfig};
    use tracing::{info, warn};

    use super::{BlockSource, RawMidnightTx, RawSubstrateBlock, SyncError, TxKind};

    // Include the subxt-generated runtime bindings (from build.rs)
    include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

    const SUBSCRIPTION_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);

    pub struct SubxtBlockSource {
        ws_url: String,
        /// How many blocks to fetch concurrently in each backfill batch.
        backfill_batch_size: usize,
        /// Lazily-initialized, reused WebSocket connection.
        conn: tokio::sync::OnceCell<(OnlineClient<SubstrateConfig>, RpcClient)>,
    }

    impl SubxtBlockSource {
        pub fn new(ws_url: String, backfill_batch_size: usize) -> Self {
            Self {
                ws_url,
                backfill_batch_size,
                conn: tokio::sync::OnceCell::new(),
            }
        }

        /// Return a shared reference to the cached connection, creating it on
        /// first call.  Both `OnlineClient` and `RpcClient` are cheaply
        /// cloneable (`Arc` inside), and the reconnecting RPC client handles
        /// transport-level reconnections automatically.
        async fn connection(
            &self,
        ) -> Result<&(OnlineClient<SubstrateConfig>, RpcClient), SyncError> {
            self.conn
                .get_or_try_init(|| async {
                    let rpc = RpcClient::builder()
                        .retry_policy(
                            ExponentialBackoff::from_millis(10)
                                .max_delay(Duration::from_secs(30))
                                .take(20),
                        )
                        .build(self.ws_url.clone())
                        .await
                        .map_err(|e| {
                            SyncError::Connection(format!("RPC connect failed: {e}"))
                        })?;

                    let client =
                        OnlineClient::<SubstrateConfig>::from_rpc_client(rpc.clone())
                            .await
                            .map_err(|e| {
                                SyncError::Connection(format!("OnlineClient failed: {e}"))
                            })?;

                    Ok((client, rpc))
                })
                .await
        }

        /// Decode extrinsics and extract midnight transactions + timestamp.
        ///
        /// This mirrors the midnight-indexer's `make_block_details` pattern:
        /// decode each extrinsic via `as_root_extrinsic::<Call>()`, match on
        /// `Midnight::send_mn_transaction` and `MidnightSystem::send_mn_system_transaction`.
        fn extract_block_details(
            extrinsics: &Extrinsics<SubstrateConfig, OnlineClient<SubstrateConfig>>,
        ) -> Result<(Vec<RawMidnightTx>, Option<u64>), SyncError> {
            use midnight_runtime::runtime_types::{
                pallet_midnight::pallet::Call::send_mn_transaction,
                pallet_midnight_system::pallet::Call::send_mn_system_transaction,
            };
            use midnight_runtime::{Call, timestamp};

            let mut midnight_txs = Vec::new();
            let mut timestamp_val = None;

            for extrinsic in extrinsics.iter() {
                let call = match extrinsic.as_root_extrinsic::<Call>() {
                    Ok(call) => call,
                    Err(e) => {
                        warn!(error = %e, "failed to decode extrinsic, skipping");
                        continue;
                    }
                };

                match call {
                    Call::Timestamp(timestamp::Call::set { now }) => {
                        timestamp_val = Some(now);
                    }
                    Call::Midnight(send_mn_transaction { midnight_tx }) => {
                        midnight_txs.push(RawMidnightTx {
                            kind: TxKind::Regular,
                            raw: midnight_tx,
                        });
                    }
                    Call::MidnightSystem(send_mn_system_transaction {
                        midnight_system_tx,
                    }) => {
                        midnight_txs.push(RawMidnightTx {
                            kind: TxKind::System,
                            raw: midnight_system_tx,
                        });
                    }
                    _ => {} // consensus, session, etc.
                }
            }

            Ok((midnight_txs, timestamp_val))
        }

        async fn block_at_number(
            client: &OnlineClient<SubstrateConfig>,
            rpc: &RpcClient,
            number: u64,
        ) -> Result<RawSubstrateBlock, SyncError> {
            let legacy = LegacyRpcMethods::<SubstrateConfig>::new(rpc.clone().into());

            let hash = legacy
                .chain_get_block_hash(Some(number.into()))
                .await
                .map_err(|e| SyncError::Connection(format!("block hash lookup: {e}")))?
                .ok_or(SyncError::BlockNotFound(number))?;

            let block = client
                .blocks()
                .at(hash)
                .await
                .map_err(|e| SyncError::Connection(format!("block fetch: {e}")))?;

            let extrinsics = block
                .extrinsics()
                .await
                .map_err(|e| SyncError::Decode(format!("extrinsics: {e}")))?;

            let (midnight_txs, timestamp) = Self::extract_block_details(&extrinsics)?;
            let header = block.header();

            Ok(RawSubstrateBlock {
                number: header.number as u64,
                hash: hash.0,
                parent_hash: header.parent_hash.0,
                midnight_txs,
                timestamp,
            })
        }
    }

    #[async_trait::async_trait]
    impl BlockSource for SubxtBlockSource {
        async fn subscribe_finalized(
            &self,
            from_block: Option<u64>,
        ) -> Result<
            Pin<
                Box<
                    dyn futures_core::Stream<Item = Result<RawSubstrateBlock, SyncError>>
                        + Send
                        + 'static,
                >,
            >,
            SyncError,
        > {
            let (client, rpc) = self.connection().await?;
            let (client, rpc) = (client.clone(), rpc.clone());
            let batch_size = self.backfill_batch_size;

            // Get the current finalized tip
            let latest = client
                .blocks()
                .at_latest()
                .await
                .map_err(|e| SyncError::Connection(format!("latest block: {e}")))?;
            let tip_number = latest.number() as u64;

            let start = from_block.unwrap_or(tip_number);

            info!(start, tip_number, backfill_batch_size = batch_size, "starting block source");

            let stream = async_stream::try_stream! {
                // Phase 1: Gap-fill — fetch historical blocks in batches.
                // Each batch fires all requests concurrently, then yields
                // them in order before starting the next batch.
                if start <= tip_number {
                    let numbers: Vec<u64> = (start..=tip_number).collect();
                    for chunk in numbers.chunks(batch_size) {
                        let futs: Vec<_> = chunk
                            .iter()
                            .map(|&n| Self::block_at_number(&client, &rpc, n))
                            .collect();

                        let results = futures_util::future::join_all(futs).await;

                        for result in results {
                            let block = result?;
                            if block.number % 1000 == 0 {
                                info!(block = block.number, "backfill progress");
                            }
                            yield block;
                        }
                    }
                }

                // Phase 2: Subscribe to live finalized blocks
                let mut last_height: Option<u64> = Some(tip_number);
                let mut sub = client
                    .blocks()
                    .subscribe_finalized()
                    .await
                    .map_err(|e| SyncError::Connection(format!("subscribe: {e}")))?;

                loop {
                    let next = tokio::time::timeout(
                        SUBSCRIPTION_RECOVERY_TIMEOUT,
                        sub.next(),
                    )
                    .await;

                    match next {
                        Ok(Some(Ok(block))) => {
                            let height = block.number() as u64;

                            // Skip duplicates (may occur after reconnect)
                            if Some(height) <= last_height {
                                warn!(height, "skipping duplicate block");
                                continue;
                            }
                            last_height = Some(height);

                            let extrinsics = block
                                .extrinsics()
                                .await
                                .map_err(|e| SyncError::Decode(format!("extrinsics: {e}")))?;

                            let (midnight_txs, timestamp) =
                                Self::extract_block_details(&extrinsics)?;

                            let header = block.header();

                            yield RawSubstrateBlock {
                                number: height,
                                hash: block.hash().0,
                                parent_hash: header.parent_hash.0,
                                midnight_txs,
                                timestamp,
                            };
                        }
                        Ok(Some(Err(e))) => {
                            // Filter reconnect errors
                            let err_str = e.to_string();
                            if err_str.contains("DisconnectedWillReconnect") {
                                warn!("node disconnected, reconnecting");
                                continue;
                            }
                            Err(SyncError::Connection(err_str))?;
                        }
                        Ok(None) => {
                            Err(SyncError::SubscriptionEnded)?;
                        }
                        Err(_timeout) => {
                            warn!("subscription stuck, re-subscribing");
                            sub = client
                                .blocks()
                                .subscribe_finalized()
                                .await
                                .map_err(|e| {
                                    SyncError::Connection(format!("re-subscribe: {e}"))
                                })?;
                        }
                    }
                }
            };

            Ok(Box::pin(stream))
        }

        async fn fetch_block(&self, number: u64) -> Result<RawSubstrateBlock, SyncError> {
            let (client, rpc) = self.connection().await?;
            Self::block_at_number(client, rpc, number).await
        }

        async fn fetch_blocks(
            &self,
            numbers: &[u64],
        ) -> Result<Vec<RawSubstrateBlock>, SyncError> {
            if numbers.is_empty() {
                return Ok(Vec::new());
            }

            let (client, rpc) = self.connection().await?;

            // Fetch all blocks concurrently over the shared connection,
            // bounded by backfill_batch_size.
            let concurrency = self.backfill_batch_size;
            futures_util::stream::iter(numbers.iter().copied())
                .map(|n| Self::block_at_number(client, rpc, n))
                .buffered(concurrency)
                .try_collect()
                .await
        }
    }
}

#[cfg(feature = "subxt-sync")]
pub use subxt_source::SubxtBlockSource;
