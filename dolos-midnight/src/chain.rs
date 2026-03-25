use std::collections::HashMap;
use std::sync::Arc;

use dolos_core::{
    mempool, Block, BlockSlot, ChainError, ChainLogic, ChainPoint, Cbor, Domain, EraCbor,
    Genesis, RawBlock, RawUtxoMap, TxOrder, TxoRef, UndoBlockData,
};

use crate::model::{MidnightDelta, MidnightEntity};
use crate::work::{MidnightWorkBuffer, MidnightWorkUnit};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum MidnightError {
    #[error("decode error: {0}")]
    Decode(String),

    #[error("sync error: {0}")]
    Sync(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Genesis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MidnightGenesis {
    pub ws_url: String,
    pub network_name: String,
}

impl Genesis for MidnightGenesis {}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct MidnightChainConfig;

// ---------------------------------------------------------------------------
// Block wrapper
// ---------------------------------------------------------------------------

/// A decoded Midnight Substrate block.
///
/// The raw bytes stored in WAL/Archive are a bincode-serialized
/// `RawSubstrateBlock`. This wrapper holds the decoded form plus the
/// original bytes for the `Block` trait.
#[derive(Debug, Clone)]
pub struct MidnightBlock {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub raw_bytes: RawBlock,
}

impl Block for MidnightBlock {
    fn depends_on(&self, _loaded: &mut RawUtxoMap) -> Vec<TxoRef> {
        // Midnight finalized blocks don't have UTxO-style dependencies
        // in the Cardano sense. Inputs are tracked via entities.
        Vec::new()
    }

    fn slot(&self) -> BlockSlot {
        self.number
    }

    fn hash(&self) -> dolos_core::BlockHash {
        dolos_core::BlockHash::new(self.hash)
    }

    fn raw(&self) -> RawBlock {
        self.raw_bytes.clone()
    }
}

// ---------------------------------------------------------------------------
// ChainLogic
// ---------------------------------------------------------------------------

pub struct MidnightLogic {
    _config: MidnightChainConfig,
    work_buffer: MidnightWorkBuffer,
}

impl ChainLogic for MidnightLogic {
    type Config = MidnightChainConfig;
    type Block = MidnightBlock;
    type Entity = MidnightEntity;
    type Utxo = (); // not used for PoC
    type Delta = MidnightDelta;
    type Genesis = MidnightGenesis;
    type ChainSpecificError = MidnightError;
    type WorkUnit<D: Domain<
        Chain = Self,
        Entity = Self::Entity,
        EntityDelta = Self::Delta,
        ChainSpecificError = Self::ChainSpecificError,
    >> = MidnightWorkUnit;

    fn initialize<D: Domain>(
        config: Self::Config,
        state: &D::State,
        _genesis: Self::Genesis,
    ) -> Result<Self, ChainError<Self::ChainSpecificError>> {
        use dolos_core::StateStore;

        let cursor = state.read_cursor().map_err(ChainError::StateError)?;

        let work_buffer = match cursor {
            Some(point) => MidnightWorkBuffer::Synced(point),
            None => MidnightWorkBuffer::Empty,
        };

        Ok(Self {
            _config: config,
            work_buffer,
        })
    }

    fn can_receive_block(&self) -> bool {
        matches!(
            self.work_buffer,
            MidnightWorkBuffer::Empty | MidnightWorkBuffer::Synced(_)
        )
    }

    fn receive_block(
        &mut self,
        raw: RawBlock,
    ) -> Result<BlockSlot, ChainError<Self::ChainSpecificError>> {
        if !self.can_receive_block() {
            return Err(ChainError::CantReceiveBlock(raw));
        }

        // Decode the block from the raw bytes (bincode-serialized RawSubstrateBlock)
        let substrate_block: crate::block_source::RawSubstrateBlock =
            bincode::deserialize(&raw).map_err(|e| {
                ChainError::ChainSpecific(MidnightError::Decode(format!(
                    "failed to deserialize substrate block: {e}"
                )))
            })?;

        let slot = substrate_block.number;

        let block = MidnightBlock {
            number: substrate_block.number,
            hash: substrate_block.hash,
            parent_hash: substrate_block.parent_hash,
            raw_bytes: raw,
        };

        self.work_buffer = MidnightWorkBuffer::PendingBlock(block);

        Ok(slot)
    }

    fn pop_work<D>(&mut self, _domain: &D) -> Option<Self::WorkUnit<D>>
    where
        D: Domain<
            Chain = Self,
            Entity = Self::Entity,
            EntityDelta = Self::Delta,
            ChainSpecificError = Self::ChainSpecificError,
            Genesis = Self::Genesis,
        >,
    {
        match std::mem::replace(&mut self.work_buffer, MidnightWorkBuffer::Empty) {
            MidnightWorkBuffer::Empty => None,
            MidnightWorkBuffer::Synced(_) => None,
            MidnightWorkBuffer::PendingBlock(block) => {
                let point = block.point();
                self.work_buffer = MidnightWorkBuffer::Synced(point);
                Some(MidnightWorkUnit::Roll(Box::new(
                    crate::work::RollWorkUnit::new(block),
                )))
            }
        }
    }

    fn compute_undo(
        _block: &Cbor,
        _inputs: &HashMap<TxoRef, Arc<EraCbor>>,
        point: ChainPoint,
    ) -> Result<UndoBlockData, ChainError<Self::ChainSpecificError>> {
        // Substrate finalized blocks cannot be rolled back (GRANDPA finality).
        // Provide a conservative no-op undo.
        Ok(UndoBlockData {
            utxo_delta: Default::default(),
            index_delta: dolos_core::IndexDelta {
                cursor: point,
                ..Default::default()
            },
            tx_hashes: Vec::new(),
        })
    }

    fn decode_utxo(
        &self,
        _utxo: Arc<EraCbor>,
    ) -> Result<Self::Utxo, ChainError<Self::ChainSpecificError>> {
        // Not used for Midnight PoC
        Ok(())
    }

    fn mutable_slots(_domain: &impl Domain<Genesis = Self::Genesis>) -> BlockSlot {
        // Substrate has instant finality via GRANDPA, so there are
        // no mutable slots. Use a small window for housekeeping.
        100
    }

    fn tx_produced_utxos(_era_body: &EraCbor) -> Vec<(TxoRef, EraCbor)> {
        // Midnight doesn't use Cardano-style UTxO refs at the core level.
        // UTxO tracking happens through entity deltas instead.
        Vec::new()
    }

    fn tx_consumed_ref(_era_body: &EraCbor) -> Vec<TxoRef> {
        Vec::new()
    }

    fn find_tx_in_block(
        _block: &[u8],
        _tx_hash: &[u8],
    ) -> Result<Option<(EraCbor, TxOrder)>, Self::ChainSpecificError> {
        // Not implemented for PoC
        Ok(None)
    }

    fn validate_tx<D: Domain<ChainSpecificError = Self::ChainSpecificError>>(
        &self,
        _cbor: &[u8],
        _utxos: &dolos_core::MempoolAwareUtxoStore<D>,
        _tip: Option<ChainPoint>,
        _genesis: &Self::Genesis,
    ) -> Result<mempool::MempoolTx, ChainError<Self::ChainSpecificError>> {
        Err(ChainError::ChainSpecific(MidnightError::Internal(
            "tx validation not supported in midnight PoC".into(),
        )))
    }
}
