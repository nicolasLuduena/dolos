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
    Block, BlockHash, BlockSlot, Bytes, ChainError, ChainLogic, ChainPoint, Domain, Entity,
    EntityDelta, EntityKey, EntityValue, Genesis, MempoolAwareUtxoStore, MempoolTx, Namespace,
    NsKey, RawBlock, RawData, RawUtxoMap, StateSchema, TxHash, TxoRef, UndoBlockData, WorkUnit,
    DomainError, TipEvent,
};
use serde::{Deserialize, Serialize};
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

/// A decoded Midnight block, ready for ledger processing.
///
/// TODO: replace the raw-bytes fields with a proper in-memory Midnight block
/// representation once a Midnight codec library is available (analogous to
/// pallas `MultiEraBlock` for Cardano).
pub struct MidnightBlock {
    /// Raw serialised block bytes (kept for archive storage).
    pub raw: RawBlock,
    /// The slot number carried in the block header.
    ///
    /// TODO: extract from the decoded block header.
    pub slot: BlockSlot,
    /// The block hash.
    ///
    /// TODO: compute using the Midnight block hashing algorithm.
    pub hash: BlockHash,
}

impl Block for MidnightBlock {
    /// Returns all UTxO references consumed by transactions in this block.
    ///
    /// These are loaded from storage before `compute()` runs so that
    /// transaction validation can resolve inputs.
    ///
    /// TODO: decode the raw block and extract every transaction input as a
    /// `TxoRef(tx_hash, output_index)`.
    fn depends_on(&self, _loaded: &mut RawUtxoMap) -> Vec<TxoRef> {
        todo!("decode Midnight block and return every consumed TxoRef")
    }

    fn slot(&self) -> BlockSlot {
        self.slot
    }

    fn hash(&self) -> BlockHash {
        self.hash
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
    /// Decode an entity from its raw storage bytes.
    ///
    /// TODO: implement using the Midnight-specific encoding.
    /// Cardano uses minicbor; Midnight may use SCALE or a bespoke codec.
    fn decode_entity(_ns: Namespace, _value: &EntityValue) -> Result<Self, ChainError> {
        todo!("decode MidnightEntity from namespace and raw bytes")
    }

    /// Encode an entity into its storage bytes.
    ///
    /// Returns `(namespace, encoded_bytes)`.
    ///
    /// TODO: implement the inverse of `decode_entity`.
    fn encode_entity(_value: &Self) -> (Namespace, EntityValue) {
        todo!("encode MidnightEntity to (namespace, raw bytes)")
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightDelta {
    /// Namespace of the entity being modified.
    ///
    /// TODO: set to the appropriate `MidnightEntity::NS_*` constant.
    pub ns: &'static str,

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
    pub fn placeholder(ns: &'static str, key: EntityKey) -> Self {
        Self { ns, key }
    }
}

impl EntityDelta for MidnightDelta {
    type Entity = MidnightEntity;

    fn key(&self) -> NsKey {
        NsKey(self.ns, self.key.clone())
    }

    /// Apply this delta to the entity.
    ///
    /// Set `*entity = Some(...)` to upsert; leave it as `None` to delete.
    ///
    /// Also capture any state needed by `undo` (e.g. store the previous value).
    ///
    /// TODO: implement for every delta variant.
    fn apply(&mut self, _entity: &mut Option<MidnightEntity>) {
        todo!("apply MidnightDelta to entity (upsert or delete)")
    }

    /// Reverse a previously applied delta (called during chain rollback).
    ///
    /// Must assume `apply` was already called and restore the entity to its
    /// pre-apply state.
    ///
    /// TODO: implement the inverse of `apply` for every delta variant.
    fn undo(&self, _entity: &mut Option<MidnightEntity>) {
        todo!("undo MidnightDelta — restore entity to its pre-apply state")
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
/// Cardano also has `Rupd` (rewards pre-computation), `Ewrap` (epoch wrap-up),
/// and `Estart` (epoch-start / era-transition) — add Midnight equivalents as
/// needed.
pub enum MidnightWorkUnit {
    /// Bootstrap the ledger from the Midnight genesis configuration.
    ///
    /// TODO: implement genesis block / initial-UTxO bootstrapping.
    Genesis,

    /// Process a block (or batch of blocks) from the chain.
    ///
    /// TODO: implement UTxO-set update, entity-delta computation, index
    /// updates, and WAL logging for normal block application.
    Roll {
        /// Raw block bytes queued by [`MidnightLogic::receive_block`].
        ///
        /// TODO: replace with a decoded `MidnightBlock` once `receive_block`
        /// is implemented, so the decode cost is paid once.
        raw: RawBlock,
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

    /// Load data from storage that `compute()` will need.
    ///
    /// Fetch resolved inputs, protocol parameters, etc. here — `compute()`
    /// must be I/O-free.
    ///
    /// TODO: implement per variant (query state/archive as required).
    fn load(&mut self, _domain: &D) -> Result<(), DomainError> {
        match self {
            Self::Genesis => todo!("load required storage data for midnight_genesis"),
            Self::Roll { .. } => todo!("load required storage data for midnight_roll"),
        }
    }

    /// Perform the CPU-intensive ledger computation.
    ///
    /// No storage I/O is allowed here; everything must be loaded in `load()`.
    ///
    /// TODO: decode the block, validate transactions, compute UTxO deltas and
    /// entity deltas.
    fn compute(&mut self) -> Result<(), DomainError> {
        match self {
            Self::Genesis => todo!("compute ledger bootstrap for midnight_genesis"),
            Self::Roll { .. } => todo!("compute ledger transition for midnight_roll"),
        }
    }

    /// Persist entity deltas and UTxO-set changes to the state store.
    ///
    /// TODO: write the deltas produced by `compute()` using a
    /// `domain.state().start_writer()` transaction.
    fn commit_state(&mut self, _domain: &D) -> Result<(), DomainError> {
        match self {
            Self::Genesis => todo!("commit_state for midnight_genesis"),
            Self::Roll { .. } => todo!("commit_state for midnight_roll"),
        }
    }

    /// Persist the raw block to the archive store.
    ///
    /// TODO: write the block bytes via `domain.archive()`.
    fn commit_archive(&mut self, _domain: &D) -> Result<(), DomainError> {
        match self {
            Self::Genesis => todo!("commit_archive for midnight_genesis"),
            Self::Roll { .. } => todo!("commit_archive for midnight_roll"),
        }
    }

    /// Produce tip-change notifications for downstream subscribers.
    ///
    /// TODO: return `TipEvent::Apply(point, raw_block)` after each committed
    /// block (and `TipEvent::Mark` at origin / epoch boundaries if applicable).
    fn tip_events(&self) -> Vec<TipEvent> {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Chain logic
// ---------------------------------------------------------------------------

/// Midnight-specific [`ChainLogic`] implementation.
///
/// This is the primary integration point between the generic Dolos
/// infrastructure and the Midnight blockchain.  An instance lives behind the
/// `RwLock<Chain>` in every `Domain` implementation.
///
/// ## Lifecycle
///
/// 1. [`MidnightLogic::initialize`] — called once at startup.
/// 2. [`MidnightLogic::can_receive_block`] — checked before each block is fed.
/// 3. [`MidnightLogic::receive_block`] — queues a raw block for processing.
/// 4. [`MidnightLogic::pop_work`] — returns the next [`MidnightWorkUnit`] to run.
///
/// The executor in `dolos-core` drives this loop and calls the work-unit
/// pipeline for each item returned by `pop_work`.
pub struct MidnightLogic {
    /// Midnight node configuration.
    pub config: MidnightConfig,

    /// The next block waiting to be turned into a [`MidnightWorkUnit`].
    ///
    /// TODO: replace with a proper work buffer once the Midnight block
    /// pipeline is fleshed out (analogous to Cardano's `WorkBuffer` which
    /// can queue multiple blocks and emit Genesis / Rupd / Ewrap / Estart
    /// work units at the right chain positions).
    pending_block: Option<RawBlock>,
}

impl ChainLogic for MidnightLogic {
    type Config = MidnightConfig;
    type Block = MidnightBlock;
    type Entity = MidnightEntity;
    type Utxo = MidnightUtxo;
    type Delta = MidnightDelta;

    type WorkUnit<D: Domain<Chain = Self, Entity = Self::Entity, EntityDelta = Self::Delta>> =
        MidnightWorkUnit;

    /// Initialize Midnight chain logic.
    ///
    /// TODO: load the Midnight genesis configuration from `config`, determine
    /// the current sync cursor from `state`, and populate any cached data
    /// structures (era summary, stability window, etc.).
    ///
    /// NOTE: the `genesis` parameter currently carries Cardano genesis files.
    /// Once `dolos-core` is further generalised, this will be replaced by a
    /// chain-agnostic genesis representation (or moved into `Config`).
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

    /// Returns `true` when no block is queued and a new one can be accepted.
    fn can_receive_block(&self) -> bool {
        self.pending_block.is_none()
    }

    /// Accept a raw block and queue it for processing.
    ///
    /// Returns the slot of the block on success.
    ///
    /// TODO: decode the block header to extract the slot number, then store
    /// the raw bytes in `self.pending_block`.
    fn receive_block(&mut self, _raw: RawBlock) -> Result<BlockSlot, ChainError> {
        todo!("decode Midnight block header to extract slot, queue block in pending_block")
    }

    /// Return the next work unit to execute, or `None` if none is ready.
    ///
    /// Currently emits a `Roll` work unit for each queued block.
    ///
    /// TODO: extend to emit `Genesis` on first run, and any Midnight-specific
    /// epoch-boundary work units at the appropriate chain positions.
    fn pop_work<D>(&mut self, _domain: &D) -> Option<MidnightWorkUnit>
    where
        D: Domain<Chain = Self, Entity = Self::Entity, EntityDelta = Self::Delta>,
    {
        self.pending_block
            .take()
            .map(|raw| MidnightWorkUnit::Roll { raw })
    }

    /// Compute the data needed to roll back a committed block.
    ///
    /// `inputs` is the resolved UTxO map saved in the WAL at commit time.
    ///
    /// TODO: decode the Midnight block, identify every UTxO produced and
    /// consumed, and construct the inverse `UtxoSetDelta`, `IndexDelta`, and
    /// `tx_hashes` list that the rollback executor requires.
    fn compute_undo(
        _block: &Bytes,
        _inputs: &HashMap<TxoRef, Arc<RawData>>,
        _point: ChainPoint,
    ) -> Result<UndoBlockData, ChainError> {
        todo!("compute undo data for Midnight block rollback")
    }

    /// Decode a raw stored UTxO into a [`MidnightUtxo`].
    ///
    /// TODO: implement using the Midnight output encoding.
    ///
    /// NOTE: this method is marked for removal from [`ChainLogic`] in a
    /// future `dolos-core` refactor (see TODO comment in core/src/lib.rs).
    fn decode_utxo(&self, _utxo: Arc<RawData>) -> Result<MidnightUtxo, ChainError> {
        todo!("decode RawData into MidnightUtxo using Midnight output encoding")
    }

    /// Returns the number of mutable (rollback-able) slots.
    ///
    /// TODO: derive from the Midnight security parameter *k*.
    ///
    /// NOTE: this method is marked for removal from [`ChainLogic`] in a
    /// future `dolos-core` refactor (see TODO comment in core/src/lib.rs).
    fn mutable_slots(_domain: &impl Domain) -> BlockSlot {
        todo!("return the Midnight stability window (security parameter k in slots)")
    }

    /// Extract UTxO outputs produced by a transaction.
    ///
    /// Returns `(output_index, raw_output_bytes)` for each output.
    ///
    /// TODO: decode the transaction with the Midnight codec and extract
    /// every output, encoding each as `RawData(protocol_version, bytes)`.
    fn tx_produces(_raw: &RawData) -> Vec<(u32, RawData)> {
        todo!("decode Midnight tx and return (index, RawData) for each produced output")
    }

    /// Extract UTxO inputs consumed by a transaction.
    ///
    /// Returns a `TxoRef(tx_hash, output_index)` for every consumed input.
    ///
    /// TODO: decode the transaction with the Midnight codec and extract
    /// every input reference.
    fn tx_consumes(_raw: &RawData) -> Vec<TxoRef> {
        todo!("decode Midnight tx and return TxoRef for each consumed input")
    }

    /// Compute the hash of a transaction from its raw bytes.
    ///
    /// TODO: implement using Midnight's transaction ID algorithm.
    fn tx_hash(_raw: &RawData) -> Option<TxHash> {
        todo!("hash a Midnight transaction to produce its TxHash")
    }

    /// Validate a transaction against the current ledger state.
    ///
    /// On success, returns a [`MempoolTx`] ready to be added to the mempool.
    ///
    /// TODO: implement Midnight transaction validation:
    ///   - Structural / phase-1 checks (well-formedness, fee adequacy, …)
    ///   - ZK proof verification (phase-2 equivalent)
    ///   - UTxO availability (inputs exist and are unspent)
    ///
    /// NOTE: the `genesis` parameter currently carries Cardano-specific data
    /// and will be generalised in a future `dolos-core` refactor.
    fn validate_tx<D: Domain>(
        &self,
        _cbor: &[u8],
        _utxos: &MempoolAwareUtxoStore<D>,
        _tip: Option<ChainPoint>,
        _genesis: &Genesis,
    ) -> Result<MempoolTx, ChainError> {
        todo!("validate a Midnight transaction against current ledger state")
    }
}

// ---------------------------------------------------------------------------
// Free functions (public API)
// ---------------------------------------------------------------------------

/// Returns the number of mutable slots for the Midnight network.
///
/// This free function is the counterpart of `dolos_cardano::mutable_slots`.
/// It should be called from a `MidnightDomainAdapter::stability_window()`
/// implementation (analogous to how `DomainAdapter` calls the Cardano version).
///
/// TODO: derive from the Midnight genesis / security parameter *k*.
pub fn mutable_slots(_genesis: &Genesis) -> BlockSlot {
    todo!("return the Midnight stability window derived from security parameter k")
}

/// Returns the state-store schema for Midnight entities.
///
/// Register one entry per namespace used by [`MidnightEntity`] variants,
/// mapping each namespace to its storage type (`KeyValue` or `KeyMultiValue`).
///
/// The Cardano analogue is `CardanoEntity::schema()`.
///
/// TODO: insert namespace → type mappings once entity variants are defined,
/// e.g.: `schema.insert(MidnightEntity::NS_PLACEHOLDER, NamespaceType::KeyValue);`
pub fn state_schema() -> StateSchema {
    StateSchema::default()
}
