//! Midnight chain logic implementation for Dolos.
//!
//! This crate provides the [`MidnightLogic`] implementation of the
//! [`dolos_core::ChainLogic`] trait, connecting the generic Dolos node
//! infrastructure to Midnight-specific block processing.
//!
//! # Architecture
//!
//! The main types mirror the Cardano crate structure:
//!
//! | Midnight type        | Cardano equivalent        | Role                                |
//! |----------------------|---------------------------|-------------------------------------|
//! | [`MidnightConfig`]   | `CardanoConfig`           | Chain-specific node configuration   |
//! | [`MidnightBlock`]    | `OwnedMultiEraBlock`      | Decoded block ready for processing  |
//! | [`MidnightEntity`]   | `CardanoEntity`           | Ledger entity stored in state DB    |
//! | [`MidnightDelta`]    | `CardanoDelta`            | Reversible entity-level change      |
//! | [`MidnightUtxo`]     | `OwnedMultiEraOutput`     | Decoded UTxO output                 |
//! | [`MidnightWorkUnit`] | `CardanoWorkUnit`         | Unit of ledger work (ETL step)      |
//! | [`MidnightLogic`]    | `CardanoLogic`            | `ChainLogic` impl (entry point)     |
//!
//! # Status
//!
//! This crate is a skeleton. All items marked `todo!()` represent
//! Midnight-specific business logic that still needs to be implemented.
//! The scaffolding compiles and integrates cleanly with `dolos-core`; only
//! the Midnight domain knowledge is missing.

// Placeholder fields and methods are intentional in this skeleton crate.
#![allow(dead_code)]

use dolos_core::{
    archive::{ArchiveStore, ArchiveWriter},
    state::{StateStore, StateWriter},
    Block, BlockHash, BlockSlot, Bytes, ChainError, ChainLogic, ChainPoint, Domain, DomainError,
    Entity, EntityDelta, EntityKey, EntityValue, Genesis, MempoolAwareUtxoStore, MempoolTx,
    Namespace, NsKey, RawBlock, RawData, RawUtxoMap, StateSchema, TipEvent, TxHash, TxoRef,
    UndoBlockData, WorkUnit,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use sp_runtime::{generic, traits::BlakeTwo256};
use std::{collections::HashMap, sync::Arc};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Midnight-specific node configuration.
///
/// This is provided to [`MidnightLogic::initialize`] during startup.
///
/// TODO: populate with real fields once the Midnight genesis / config schema
/// is defined (e.g. network ID, genesis UTXO set path, security parameter k).
#[derive(Debug, Clone, Default)]
pub struct MidnightConfig {
    /// Network identifier (testnet, mainnet, …).
    ///
    /// TODO: replace with a proper network enum once Midnight networks are named.
    pub network_id: u32,
}

// ---------------------------------------------------------------------------
// Block
// ---------------------------------------------------------------------------

/// Midnight uses u32 block numbers (matches the Midnight node runtime).
pub type BlockNumber = u32;
pub type BlockHeader = generic::Header<BlockNumber, BlakeTwo256>;

/// Opaque block type — matches `midnight_node_runtime::opaque::Block`.
pub type OpaqueBlock = generic::Block<BlockHeader, sp_runtime::OpaqueExtrinsic>;

/// A decoded Midnight block, ready for ledger processing.
pub struct MidnightBlock {
    /// Raw serialised block bytes (kept for archive storage).
    pub raw: RawBlock,
    /// The slot number carried in the block header.
    pub header: BlockHeader,
}

impl Block for MidnightBlock {
    /// Returns all UTxO references consumed by transactions in this block.
    ///
    /// These are loaded from storage before `compute()` runs so that
    /// transaction validation can resolve inputs.
    ///
    /// `TxoRef(tx_hash, output_index)`.
    fn depends_on(&self, _loaded: &mut RawUtxoMap) -> Vec<TxoRef> {
        // TODO: decode Midnight block and return every consumed TxoRef
        vec![]
    }

    fn slot(&self) -> BlockSlot {
        self.header.number.into()
    }

    fn hash(&self) -> BlockHash {
        self.header.hash().as_bytes().into()
    }

    fn raw(&self) -> RawBlock {
        self.raw.clone()
    }
}

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// Midnight ledger entities stored in the state database.
///
/// Each variant maps to a distinct namespace in the key-value state store.
///
/// TODO: define one variant per entity type tracked by the Midnight ledger
/// (e.g. accounts, UTxO metadata, protocol parameters, epoch state).
/// The Cardano analogue is the `CardanoEntity` enum in `dolos-cardano`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MidnightEntity {
    /// Placeholder — replace with real Midnight entity variants.
    Placeholder(Vec<u8>),
}

impl MidnightEntity {
    /// State-store namespace for placeholder entities.
    ///
    /// TODO: define a proper namespace constant for each entity variant
    /// (analogous to `EpochState::NS`, `UtxoState::NS`, etc. in Cardano).
    pub const NS_PLACEHOLDER: Namespace = "midnight/placeholder";
}

impl Entity for MidnightEntity {
    fn decode_entity(_ns: Namespace, value: &EntityValue) -> Result<Self, ChainError> {
        Ok(Self::Placeholder(value.clone()))
    }

    fn encode_entity(value: &Self) -> (Namespace, EntityValue) {
        match value {
            Self::Placeholder(data) => (Self::NS_PLACEHOLDER, data.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Entity Delta
// ---------------------------------------------------------------------------

/// A reversible change to a single Midnight ledger entity.
///
/// Deltas are persisted in the WAL so they must be `Serialize + Deserialize`.
/// They are applied forward during normal sync and reversed during rollback.
///
/// TODO: either make this an enum with one variant per state transition type
/// (the recommended approach, matching Cardano's `CardanoDelta`), or keep a
/// single struct and add an `op` field discriminating the change kind.
///
/// Each variant/op needs a working `apply` *and* `undo` implementation —
/// `undo` must be able to fully reverse `apply` without extra context.
///
/// The `ns` field uses `String` (not `&'static str`) so that `MidnightDelta`
/// satisfies `DeserializeOwned`, which is required by the WAL store backend.
/// Call sites convert back to `&'static str` via the known namespace constants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightDelta {
    /// Namespace of the entity being modified (stored as an owned string for
    /// serde compatibility; currently always `MidnightEntity::NS_PLACEHOLDER`).
    pub ns: String,

    /// Key of the entity being modified within its namespace.
    pub key: EntityKey,
    // TODO: add the payload fields that describe the change, e.g.:
    //   pub before: Option<Vec<u8>>,   // snapshot for undo
    //   pub after:  Option<Vec<u8>>,   // new value for apply
}

impl MidnightDelta {
    /// Construct a placeholder delta (useful for scaffolding tests).
    ///
    /// TODO: replace with typed constructors once delta variants are defined.
    pub fn placeholder(key: EntityKey) -> Self {
        Self {
            ns: MidnightEntity::NS_PLACEHOLDER.to_string(),
            key,
        }
    }
}

impl EntityDelta for MidnightDelta {
    type Entity = MidnightEntity;

    fn key(&self) -> NsKey {
        // Currently only one namespace is defined; all deltas use NS_PLACEHOLDER.
        // Extend this match once more entity variants are added.
        NsKey(MidnightEntity::NS_PLACEHOLDER, self.key.clone())
    }

    fn apply(&mut self, _entity: &mut Option<MidnightEntity>) {
        // no-op: real delta application to be implemented
    }

    fn undo(&self, _entity: &mut Option<MidnightEntity>) {
        // no-op: real rollback to be implemented
    }
}

// ---------------------------------------------------------------------------
// UTxO output
// ---------------------------------------------------------------------------

/// A decoded Midnight UTxO output ready for in-memory use.
///
/// TODO: add decoded fields (address, value, script/datum attachment, etc.)
/// once the Midnight output format is defined.  The Cardano analogue is
/// `OwnedMultiEraOutput`.
pub struct MidnightUtxo {
    /// Raw serialised output bytes (the source of truth).
    pub raw: RawData,
}

// ---------------------------------------------------------------------------
// Work units
// ---------------------------------------------------------------------------

/// All possible work units produced by [`MidnightLogic`].
///
/// Work units are the atomic ETL steps of ledger processing; each follows the
/// pipeline: `load() → compute() → commit_wal() → commit_state() →
/// commit_archive() → commit_indexes()`.
///
/// TODO: add variants as the Midnight lifecycle is defined.  At minimum you
/// will need `Genesis` (bootstrap) and `Roll` (normal block application).
pub enum MidnightWorkUnit {
    /// Bootstrap the ledger from the Midnight genesis configuration.
    Genesis,

    /// Process a block (or batch of blocks) from the chain.
    Roll {
        /// Raw block bytes queued by [`MidnightLogic::receive_block`].
        block: MidnightBlock,
    },
}

impl<D> WorkUnit<D> for MidnightWorkUnit
where
    D: Domain<Chain = MidnightLogic, Entity = MidnightEntity, EntityDelta = MidnightDelta>,
{
    fn name(&self) -> &'static str {
        match self {
            Self::Genesis => "midnight_genesis",
            Self::Roll { .. } => "midnight_roll",
        }
    }

    fn load(&mut self, _domain: &D) -> Result<(), DomainError> {
        Ok(())
    }

    fn compute(&mut self) -> Result<(), DomainError> {
        Ok(())
    }

    fn commit_state(&mut self, domain: &D) -> Result<(), DomainError> {
        match self {
            Self::Genesis => Ok(()),
            Self::Roll { block } => {
                let point = block.point();
                let writer = domain.state().start_writer()?;
                writer.set_cursor(point)?;
                writer.commit()?;
                Ok(())
            }
        }
    }

    fn commit_archive(&mut self, domain: &D) -> Result<(), DomainError> {
        match self {
            Self::Genesis => Ok(()),
            Self::Roll { block } => {
                let point = block.point();
                let raw = block.raw();
                let writer = domain.archive().start_writer()?;
                writer.apply(&point, &raw)?;
                writer.commit()?;
                Ok(())
            }
        }
    }

    fn tip_events(&self) -> Vec<TipEvent> {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Chain logic
// ---------------------------------------------------------------------------

/// Midnight-specific [`ChainLogic`] implementation.
pub struct MidnightLogic {
    /// Midnight node configuration.
    pub config: MidnightConfig,

    /// The next block waiting to be turned into a [`MidnightWorkUnit`].
    pending_block: Option<MidnightBlock>,
}

impl ChainLogic for MidnightLogic {
    type Config = MidnightConfig;
    type Block = MidnightBlock;
    type Entity = MidnightEntity;
    type Utxo = MidnightUtxo;
    type Delta = MidnightDelta;

    type WorkUnit<D: Domain<Chain = Self, Entity = Self::Entity, EntityDelta = Self::Delta>> =
        MidnightWorkUnit;

    fn initialize<D: Domain>(
        config: Self::Config,
        _state: &D::State,
        _genesis: &Genesis,
    ) -> Result<Self, ChainError> {
        Ok(Self {
            config,
            pending_block: None,
        })
    }

    fn can_receive_block(&self) -> bool {
        self.pending_block.is_none()
    }

    fn receive_block(&mut self, raw: RawBlock) -> Result<BlockSlot, ChainError> {
        let header = BlockHeader::decode(&mut raw.as_slice())
            .map_err(|_e| ChainError::CantReceiveBlock(raw.clone()))?;
        let slot = header.number.into();
        self.pending_block = Some(MidnightBlock { raw, header });

        Ok(slot)
    }

    fn pop_work<D>(&mut self, _domain: &D) -> Option<MidnightWorkUnit>
    where
        D: Domain<Chain = Self, Entity = Self::Entity, EntityDelta = Self::Delta>,
    {
        self.pending_block
            .take()
            .map(|block| MidnightWorkUnit::Roll { block })
    }

    fn compute_undo(
        _block: &Bytes,
        _inputs: &HashMap<TxoRef, Arc<RawData>>,
        _point: ChainPoint,
    ) -> Result<UndoBlockData, ChainError> {
        Ok(UndoBlockData {
            utxo_delta: Default::default(),
            index_delta: Default::default(),
            tx_hashes: vec![],
        })
    }

    fn decode_utxo(&self, utxo: Arc<RawData>) -> Result<MidnightUtxo, ChainError> {
        Ok(MidnightUtxo {
            raw: (*utxo).clone(),
        })
    }

    fn mutable_slots(_domain: &impl Domain) -> BlockSlot {
        2160u64
    }

    fn tx_produces(_raw: &RawData) -> Vec<(u32, RawData)> {
        vec![]
    }

    fn tx_consumes(_raw: &RawData) -> Vec<TxoRef> {
        vec![]
    }

    fn tx_hash(_raw: &RawData) -> Option<TxHash> {
        None
    }

    fn validate_tx<D: Domain>(
        &self,
        _cbor: &[u8],
        _utxos: &MempoolAwareUtxoStore<D>,
        _tip: Option<ChainPoint>,
        _genesis: &Genesis,
    ) -> Result<MempoolTx, ChainError> {
        Err(ChainError::Phase2EvaluationError(
            "midnight tx validation not yet implemented".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Free functions (public API)
// ---------------------------------------------------------------------------

/// Returns the number of mutable slots for the Midnight network.
///
/// TODO: derive from the Midnight genesis / security parameter *k*.
pub fn mutable_slots(_genesis: &Genesis) -> BlockSlot {
    2160u64
}

/// Returns the state-store schema for Midnight entities.
///
/// TODO: insert namespace → type mappings once entity variants are defined.
pub fn state_schema() -> StateSchema {
    StateSchema::default()
}

/// Encode a block from the Substrate JSON-RPC `chain_getBlock` response into
/// a SCALE-encoded [`OpaqueBlock`] that can be fed to [`MidnightLogic::receive_block`].
///
/// `number_hex` is the `0x`-prefixed hex block number from the JSON header.
/// `extrinsics` is the list of `0x`-prefixed hex-encoded extrinsic bytes.
///
/// Returns an error string if any hex field fails to decode.
pub fn encode_block(
    _hash: &str,
    number_hex: &str,
    extrinsics: &[String],
) -> Result<RawBlock, String> {
    let number = parse_block_number(number_hex)?;

    let header = BlockHeader {
        parent_hash: Default::default(),
        number,
        state_root: Default::default(),
        extrinsics_root: Default::default(),
        digest: Default::default(),
    };

    let opaque_extrinsics: Vec<sp_runtime::OpaqueExtrinsic> = extrinsics
        .iter()
        .map(|hex| {
            let bytes = hex_to_bytes(hex)?;
            sp_runtime::OpaqueExtrinsic::from_bytes(&bytes)
                .map_err(|e| format!("invalid extrinsic: {e}"))
        })
        .collect::<Result<_, _>>()?;

    let block = OpaqueBlock {
        header,
        extrinsics: opaque_extrinsics,
    };

    Ok(Arc::new(block.encode()))
}

/// Parse a `0x`-prefixed hex block number string into a [`BlockNumber`].
fn parse_block_number(hex: &str) -> Result<BlockNumber, String> {
    let hex = hex.trim_start_matches("0x");
    u32::from_str_radix(hex, 16).map_err(|e| format!("invalid block number {hex:?}: {e}"))
}

/// Decode a `0x`-prefixed hex string into raw bytes.
fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    let hex = hex.trim_start_matches("0x");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("invalid hex: {e}")))
        .collect()
}
