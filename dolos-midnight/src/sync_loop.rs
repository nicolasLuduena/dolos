use dolos_core::{StateStore, SyncExt};
use tracing::{debug, error, info};

use crate::block_source::{BlockSource, SyncError};
use crate::domain::MidnightDomain;

/// Run the block sync loop.
///
/// Connects to the Midnight node via `source`, subscribes to finalized blocks
/// starting from the current state cursor, and feeds each block through the
/// dolos-core work unit pipeline (WAL → state → archive → indexes).
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

    match &cursor {
        Some(p) => info!(cursor = ?p, from_block = ?from_block, "resuming sync from stored cursor"),
        None => info!("no cursor found, starting sync from genesis"),
    }

    info!("subscribing to finalized blocks");
    let mut stream = source.subscribe_finalized(from_block).await?;
    info!("subscription established, processing blocks");

    let mut blocks_processed: u64 = 0;
    let start_time = std::time::Instant::now();

    use futures_util::StreamExt;
    while let Some(result) = stream.next().await {
        let raw_block = result?;

        let slot = raw_block.number;
        let tx_count = raw_block.midnight_txs.len();
        let hash_prefix = hex::encode(&raw_block.hash[..4]);

        debug!(
            slot,
            tx_count,
            hash = %hash_prefix,
            "received block"
        );

        let raw_bytes =
            bincode::serialize(&raw_block).map_err(|e| SyncError::Decode(e.to_string()))?;

        let raw = std::sync::Arc::new(raw_bytes);

        match domain.roll_forward(raw) {
            Ok(_) => {
                blocks_processed += 1;

                if tx_count > 0 {
                    info!(
                        slot,
                        tx_count,
                        hash = %hash_prefix,
                        "processed block with transactions"
                    );
                }

                // Progress log every 100 blocks
                if blocks_processed % 100 == 0 {
                    let elapsed = start_time.elapsed();
                    let rate = blocks_processed as f64 / elapsed.as_secs_f64();
                    info!(
                        slot,
                        blocks_processed,
                        blocks_per_sec = format!("{rate:.1}"),
                        "sync progress"
                    );
                }
            }
            Err(e) => {
                error!(slot, hash = %hash_prefix, error = %e, "failed to process block");
                return Err(SyncError::Decode(format!("block processing failed: {e}")));
            }
        }
    }

    info!(blocks_processed, "sync stream ended");
    Ok(())
}
