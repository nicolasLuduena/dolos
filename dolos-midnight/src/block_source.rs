use serde::{Deserialize, Serialize};

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
    /// Raw encoded extrinsics (each is an opaque byte vec).
    pub extrinsics: Vec<Vec<u8>>,
    /// Block timestamp from the Timestamp pallet (if present).
    pub timestamp: Option<u64>,
}

// ---------------------------------------------------------------------------
// BlockSource trait — Front 2 implements this
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
///
/// Front 2 provides `SubxtBlockSource` (real WebSocket client).
/// For testing, `MockBlockSource` returns synthetic blocks.
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
}

// ---------------------------------------------------------------------------
// MockBlockSource — for testing
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
        // Return an empty stream for Phase 0
        Ok(Box::pin(futures_util::stream::empty()))
    }

    async fn fetch_block(&self, number: u64) -> Result<RawSubstrateBlock, SyncError> {
        // Return a synthetic block for Phase 0
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
            extrinsics: Vec::new(),
            timestamp: Some(number * 12_000), // ~12s block time
        })
    }
}
