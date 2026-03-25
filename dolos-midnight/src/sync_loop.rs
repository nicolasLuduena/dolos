use dolos_core::{StateStore, SyncExt};
use tracing::{error, info};

use crate::block_source::{BlockSource, SyncError};
use crate::domain::MidnightDomain;

/// Run the block sync loop.
///
/// Connects to the Midnight node via `source`, subscribes to finalized blocks
/// starting from the current state cursor, and feeds each block through the
/// dolos-core work unit pipeline (WAL → state → archive → indexes).
///
/// Front 2 provides the real `SubxtBlockSource`. This function is
/// source-agnostic — it works with any `BlockSource` impl.
pub async fn run_sync(
    domain: MidnightDomain,
    source: impl BlockSource,
) -> Result<(), SyncError> {
    let cursor = domain
        .state
        .read_cursor()
        .map_err(|e| SyncError::Connection(format!("failed to read cursor: {e}")))?;

    let from_block = cursor.as_ref().map(|p| match p {
        dolos_core::ChainPoint::Origin => 0u64,
        dolos_core::ChainPoint::Slot(slot) | dolos_core::ChainPoint::Specific(slot, _) => {
            *slot + 1
        }
    });

    info!(from_block = ?from_block, "starting sync loop");

    let mut stream = source.subscribe_finalized(from_block).await?;

    use futures_util::StreamExt;
    while let Some(result) = stream.next().await {
        let raw_block = result?;

        let slot = raw_block.number;
        let raw_bytes =
            bincode::serialize(&raw_block).map_err(|e| SyncError::Decode(e.to_string()))?;

        let raw = std::sync::Arc::new(raw_bytes);

        match domain.roll_forward(raw) {
            Ok(_) => {
                if slot % 1000 == 0 {
                    info!(slot, "synced block");
                }
            }
            Err(e) => {
                error!(slot, error = %e, "failed to process block");
                return Err(SyncError::Decode(format!("block processing failed: {e}")));
            }
        }
    }

    info!("sync stream ended");
    Ok(())
}
