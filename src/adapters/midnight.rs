//! Domain adapter for the Midnight blockchain.
//!
//! This module provides [`MidnightDomainAdapter`], the concrete implementation
//! of [`dolos_core::Domain`] for Midnight, wiring up storage backends to the
//! [`dolos_midnight::MidnightLogic`] chain logic.

use std::sync::Arc;

use dolos_core::{
    config::{StorageConfig, SyncConfig},
    *,
};
use dolos_midnight::{MidnightDelta, MidnightEntity, MidnightLogic, MidnightWorkUnit};

use super::{
    ArchiveStoreBackend, IndexStoreBackend, MempoolBackend, StateStoreBackend, TipSubscription,
    WalStoreBackend,
};

/// Type alias for the WAL store specialised for Midnight.
pub type MidnightWalAdapter = WalStoreBackend<MidnightDelta>;

/// Concrete [`Domain`] implementation that wires Midnight logic to storage backends.
#[derive(Clone)]
pub struct MidnightDomainAdapter {
    pub storage_config: Arc<StorageConfig>,
    pub sync_config: Arc<SyncConfig>,
    pub genesis: Arc<Genesis>,
    pub wal: MidnightWalAdapter,
    pub chain: Arc<std::sync::RwLock<MidnightLogic>>,
    pub state: StateStoreBackend,
    pub archive: ArchiveStoreBackend,
    pub indexes: IndexStoreBackend,
    pub mempool: MempoolBackend,
    pub tip_broadcast: tokio::sync::broadcast::Sender<TipEvent>,
}

impl MidnightDomainAdapter {
    /// Gracefully shutdown all storage backends.
    pub fn shutdown(&self) -> Result<(), DomainError> {
        self.wal.shutdown().map_err(DomainError::WalError)?;
        self.state.shutdown().map_err(DomainError::StateError)?;
        self.archive.shutdown().map_err(DomainError::ArchiveError)?;
        self.indexes.shutdown().map_err(DomainError::IndexError)?;
        Ok(())
    }
}

impl Domain for MidnightDomainAdapter {
    type Entity = MidnightEntity;
    type EntityDelta = MidnightDelta;
    type Chain = MidnightLogic;
    type WorkUnit = MidnightWorkUnit;
    type Wal = MidnightWalAdapter;
    type State = StateStoreBackend;
    type Archive = ArchiveStoreBackend;
    type Indexes = IndexStoreBackend;
    type Mempool = MempoolBackend;
    type TipSubscription = TipSubscription;

    fn genesis(&self) -> Arc<Genesis> {
        self.genesis.clone()
    }

    fn read_chain(&self) -> std::sync::RwLockReadGuard<'_, Self::Chain> {
        self.chain.read().expect("chain lock poisoned")
    }

    fn write_chain(&self) -> std::sync::RwLockWriteGuard<'_, Self::Chain> {
        self.chain.write().expect("chain lock poisoned")
    }

    fn wal(&self) -> &Self::Wal {
        &self.wal
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn archive(&self) -> &Self::Archive {
        &self.archive
    }

    fn indexes(&self) -> &Self::Indexes {
        &self.indexes
    }

    fn mempool(&self) -> &Self::Mempool {
        &self.mempool
    }

    fn storage_config(&self) -> &StorageConfig {
        &self.storage_config
    }

    fn sync_config(&self) -> &SyncConfig {
        &self.sync_config
    }

    fn watch_tip(&self, from: Option<ChainPoint>) -> Result<Self::TipSubscription, DomainError> {
        let receiver = self.tip_broadcast.subscribe();
        let replay = self.wal().iter_blocks(from, None)?.collect::<Vec<_>>();
        Ok(TipSubscription { replay, receiver })
    }

    fn notify_tip(&self, tip: TipEvent) {
        if self.tip_broadcast.receiver_count() > 0 {
            self.tip_broadcast.send(tip).unwrap();
        }
    }

    fn stability_window(&self) -> BlockSlot {
        dolos_midnight::mutable_slots(&self.genesis())
    }
}
