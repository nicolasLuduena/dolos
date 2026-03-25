use dolos_core::{ImportExt, RawBlock, StateStore};
use tracing::{debug, error, info};

use crate::block_source::{BlockSource, SyncError};
use crate::domain::MidnightDomain;

/// Run the block sync loop.
///
/// Connects to the Midnight node via `source`, subscribes to finalized blocks
/// starting from the current state cursor, and feeds batches of blocks through
/// the dolos-core import pipeline (state → archive → indexes).
///
/// Uses `ImportExt::import_blocks` which skips WAL commits — safe for
/// Midnight because GRANDPA finality means finalized blocks never roll back.
pub async fn run_sync(
    domain: MidnightDomain,
    source: impl BlockSource,
    batch_size: usize,
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

    let mut batch: Vec<RawBlock> = Vec::with_capacity(batch_size);
    let mut batch_first_slot: u64 = 0;
    let mut batch_tx_count: usize = 0;

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

        if tx_count > 0 {
            info!(
                slot,
                tx_count,
                hash = %hash_prefix,
                "block contains transactions"
            );
        }

        let raw_bytes =
            bincode::serialize(&raw_block).map_err(|e| SyncError::Decode(e.to_string()))?;

        if batch.is_empty() {
            batch_first_slot = slot;
            batch_tx_count = 0;
        }
        batch_tx_count += tx_count;
        batch.push(std::sync::Arc::new(raw_bytes));

        if batch.len() >= batch_size {
            let count = batch.len();
            let last_slot = slot;
            info!(
                from_slot = batch_first_slot,
                to_slot = last_slot,
                blocks = count,
                txs = batch_tx_count,
                "flushing batch"
            );

            match domain.import_blocks(std::mem::take(&mut batch)) {
                Ok(_) => {
                    blocks_processed += count as u64;
                    let elapsed = start_time.elapsed();
                    let rate = blocks_processed as f64 / elapsed.as_secs_f64();
                    info!(
                        tip_slot = last_slot,
                        blocks_processed,
                        blocks_per_sec = format!("{rate:.1}"),
                        "batch committed"
                    );
                }
                Err(e) => {
                    error!(
                        from_slot = batch_first_slot,
                        to_slot = last_slot,
                        error = %e,
                        "batch processing failed"
                    );
                    return Err(SyncError::Decode(format!("batch processing failed: {e}")));
                }
            }

            batch = Vec::with_capacity(batch_size);
        }
    }

    // Flush remaining blocks
    if !batch.is_empty() {
        let count = batch.len();
        info!(
            from_slot = batch_first_slot,
            blocks = count,
            txs = batch_tx_count,
            "flushing final batch"
        );

        match domain.import_blocks(batch) {
            Ok(last_slot) => {
                blocks_processed += count as u64;
                info!(tip_slot = last_slot, blocks_processed, "final batch committed");
            }
            Err(e) => {
                error!(error = %e, "final batch processing failed");
                return Err(SyncError::Decode(format!("final batch failed: {e}")));
            }
        }
    }

    info!(blocks_processed, "sync stream ended");
    Ok(())
}
