use std::sync::{Arc, RwLock};

use dolos_core::{
    config::{StorageConfig, SyncConfig},
    ChainPoint, DomainError, MempoolError, MempoolEvent, MempoolPage, MempoolStore, MempoolTx,
    MempoolTxStage, TipEvent, TxHash, TxStatus,
};

use crate::chain::{MidnightError, MidnightGenesis, MidnightLogic};
use crate::model::{MidnightDelta, MidnightEntity};
use crate::work::MidnightWorkUnit;

// ---------------------------------------------------------------------------
// StubMempool — no-op mempool (Midnight PoC doesn't need tx submission)
// ---------------------------------------------------------------------------

pub struct EmptyMempoolStream;

impl futures_core::Stream for EmptyMempoolStream {
    type Item = Result<MempoolEvent, MempoolError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(None)
    }
}

#[derive(Clone)]
pub struct StubMempool;

impl MempoolStore for StubMempool {
    type Stream = EmptyMempoolStream;

    fn receive(&self, _tx: MempoolTx) -> Result<(), MempoolError> {
        Err(MempoolError::Internal(Box::new(std::io::Error::other(
            "mempool not supported in midnight PoC",
        ))))
    }

    fn has_pending(&self) -> bool {
        false
    }

    fn peek_pending(&self) -> Vec<MempoolTx> {
        Vec::new()
    }

    fn mark_inflight(&self, _hashes: &[TxHash]) -> Result<(), MempoolError> {
        Ok(())
    }

    fn mark_acknowledged(&self, _hashes: &[TxHash]) -> Result<(), MempoolError> {
        Ok(())
    }

    fn find_inflight(&self, _tx_hash: &TxHash) -> Option<MempoolTx> {
        None
    }

    fn peek_inflight(&self) -> Vec<MempoolTx> {
        Vec::new()
    }

    fn confirm(
        &self,
        _point: &ChainPoint,
        _seen_txs: &[TxHash],
        _unseen_txs: &[TxHash],
        _finalize_threshold: u32,
        _drop_threshold: u32,
    ) -> Result<(), MempoolError> {
        Ok(())
    }

    fn check_status(&self, _tx_hash: &TxHash) -> TxStatus {
        TxStatus {
            stage: MempoolTxStage::Unknown,
            confirmations: 0,
            non_confirmations: 0,
            confirmed_at: None,
        }
    }

    fn dump_finalized(&self, _cursor: u64, _limit: usize) -> MempoolPage {
        MempoolPage {
            items: Vec::new(),
            next_cursor: None,
        }
    }

    fn subscribe(&self) -> Self::Stream {
        EmptyMempoolStream
    }
}

// ---------------------------------------------------------------------------
// TipSubscription
// ---------------------------------------------------------------------------

pub struct MidnightTipSubscription {
    receiver: tokio::sync::broadcast::Receiver<TipEvent>,
}

impl dolos_core::TipSubscription for MidnightTipSubscription {
    async fn next_tip(&mut self) -> TipEvent {
        self.receiver.recv().await.unwrap()
    }
}

// ---------------------------------------------------------------------------
// MidnightDomain — wires all stores and chain logic together
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MidnightDomain {
    pub wal: dolos_redb3::wal::RedbWalStore<MidnightDelta>,
    pub chain: Arc<RwLock<MidnightLogic>>,
    pub state: dolos_redb3::state::StateStore,
    pub archive: dolos_redb3::archive::ArchiveStore<MidnightError>,
    pub indexes: dolos_redb3::indexes::IndexStore,
    pub mempool: StubMempool,
    pub storage_config: StorageConfig,
    pub sync_config: SyncConfig,
    pub genesis: Arc<MidnightGenesis>,
    pub tip_broadcast: tokio::sync::broadcast::Sender<TipEvent>,
}

impl dolos_core::Domain for MidnightDomain {
    type Entity = MidnightEntity;
    type EntityDelta = MidnightDelta;
    type Genesis = MidnightGenesis;
    type ChainSpecificError = MidnightError;

    type Chain = MidnightLogic;
    type WorkUnit = MidnightWorkUnit;
    type Wal = dolos_redb3::wal::RedbWalStore<MidnightDelta>;
    type State = dolos_redb3::state::StateStore;
    type Archive = dolos_redb3::archive::ArchiveStore<MidnightError>;
    type Indexes = dolos_redb3::indexes::IndexStore;
    type Mempool = StubMempool;
    type TipSubscription = MidnightTipSubscription;

    fn storage_config(&self) -> &StorageConfig {
        &self.storage_config
    }

    fn sync_config(&self) -> &SyncConfig {
        &self.sync_config
    }

    fn genesis(&self) -> Arc<MidnightGenesis> {
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

    fn watch_tip(
        &self,
        _from: Option<ChainPoint>,
    ) -> Result<Self::TipSubscription, DomainError<MidnightError>> {
        let receiver = self.tip_broadcast.subscribe();
        Ok(MidnightTipSubscription { receiver })
    }

    fn notify_tip(&self, tip: TipEvent) {
        if self.tip_broadcast.receiver_count() > 0 {
            let _ = self.tip_broadcast.send(tip);
        }
    }
}
