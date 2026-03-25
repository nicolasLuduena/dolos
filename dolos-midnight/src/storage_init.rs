use std::path::Path;

use dolos_core::config::{RedbArchiveConfig, RedbIndexConfig, RedbStateConfig, RedbWalConfig};

use crate::chain::MidnightError;
use crate::model::{self, MidnightDelta};

/// Open (or create) all four redb3 stores for the Midnight domain.
///
/// Each store lives in a subdirectory under `base_path`:
///   - `wal/`     — write-ahead log for crash recovery
///   - `state/`   — current entity state (UTxOs)
///   - `chain/`   — historical blocks (archive)
///   - `index/`   — cross-cutting lookups
pub fn open_stores(
    base_path: &Path,
) -> Result<
    (
        dolos_redb3::wal::RedbWalStore<MidnightDelta>,
        dolos_redb3::state::StateStore,
        dolos_redb3::archive::ArchiveStore<MidnightError>,
        dolos_redb3::indexes::IndexStore,
    ),
    Box<dyn std::error::Error>,
> {
    std::fs::create_dir_all(base_path)?;

    let schema = model::build_schema();

    let wal_path = base_path.join("wal");
    let wal = dolos_redb3::wal::RedbWalStore::open(&wal_path, &RedbWalConfig::default())?;

    let state_path = base_path.join("state");
    let state =
        dolos_redb3::state::StateStore::open(schema.clone(), &state_path, &RedbStateConfig::default())?;

    let archive_path = base_path.join("chain");
    let archive =
        dolos_redb3::archive::ArchiveStore::open(schema, &archive_path, &RedbArchiveConfig::default())?;

    let index_path = base_path.join("index");
    let indexes = dolos_redb3::indexes::IndexStore::open(&index_path, &RedbIndexConfig::default())?;

    Ok((wal, state, archive, indexes))
}
