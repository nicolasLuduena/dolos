use std::sync::Arc;

use dolos_core::{
    Block, ChainPoint, Domain, DomainError, Entity, LogValue, TipEvent, WorkUnit,
    ArchiveStore as _, ArchiveWriter as _, StateStore, StateWriter as _,
    WalStore,
};
use tracing::{debug, info};

use crate::block_source::RawSubstrateBlock;
use crate::chain::{MidnightBlock, MidnightError};
use crate::model::{
    self, CreateShieldedUtxo, CreateUnshieldedUtxo, MidnightDelta, ShieldedUtxoState,
    UnshieldedUtxoState,
};
use crate::tx_parser::{ExtrinsicParser, ParsedBlock};

// ---------------------------------------------------------------------------
// WorkBuffer — state machine for pending work
// ---------------------------------------------------------------------------

pub enum MidnightWorkBuffer {
    /// No blocks received yet (fresh start).
    Empty,
    /// Fully synced up to this point, ready for next block.
    Synced(ChainPoint),
    /// A block has been received and is waiting to be processed.
    PendingBlock(MidnightBlock),
}

// ---------------------------------------------------------------------------
// WorkUnit enum
// ---------------------------------------------------------------------------

pub enum MidnightWorkUnit {
    Roll(Box<RollWorkUnit>),
}

impl<D> WorkUnit<D> for MidnightWorkUnit
where
    D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>,
{
    fn name(&self) -> &'static str {
        match self {
            MidnightWorkUnit::Roll(_) => "roll",
        }
    }

    fn load(&mut self, domain: &D) -> Result<(), DomainError<MidnightError>> {
        match self {
            MidnightWorkUnit::Roll(w) => w.load(domain),
        }
    }

    fn compute(&mut self) -> Result<(), DomainError<MidnightError>> {
        match self {
            MidnightWorkUnit::Roll(w) => w.compute(),
        }
    }

    fn commit_wal(&mut self, domain: &D) -> Result<(), DomainError<MidnightError>> {
        match self {
            MidnightWorkUnit::Roll(w) => w.commit_wal(domain),
        }
    }

    fn commit_state(&mut self, domain: &D) -> Result<(), DomainError<MidnightError>> {
        match self {
            MidnightWorkUnit::Roll(w) => w.commit_state(domain),
        }
    }

    fn commit_archive(&mut self, domain: &D) -> Result<(), DomainError<MidnightError>> {
        match self {
            MidnightWorkUnit::Roll(w) => w.commit_archive(domain),
        }
    }

    fn commit_indexes(&mut self, _domain: &D) -> Result<(), DomainError<MidnightError>> {
        Ok(())
    }

    fn tip_events(&self) -> Vec<TipEvent> {
        match self {
            MidnightWorkUnit::Roll(w) => w.tip_events(),
        }
    }
}

// ---------------------------------------------------------------------------
// RollWorkUnit — processes one finalized block
// ---------------------------------------------------------------------------

pub struct RollWorkUnit {
    block: MidnightBlock,
    parser: Arc<dyn ExtrinsicParser>,
    parsed: Option<ParsedBlock>,
    deltas: Vec<MidnightDelta>,
}

impl RollWorkUnit {
    pub fn new(block: MidnightBlock, parser: Arc<dyn ExtrinsicParser>) -> Self {
        Self {
            block,
            parser,
            parsed: None,
            deltas: Vec::new(),
        }
    }

    fn load<D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>>(
        &mut self,
        _domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        let slot = self.block.number;
        debug!(slot, "work: load — deserializing block");

        // Deserialize the raw block back to RawSubstrateBlock for the parser
        let substrate_block: RawSubstrateBlock =
            bincode::deserialize(&self.block.raw_bytes).map_err(|e| {
                DomainError::ChainError(dolos_core::ChainError::ChainSpecific(
                    MidnightError::Decode(format!("re-decode block: {e}")),
                ))
            })?;

        debug!(
            slot,
            midnight_txs = substrate_block.midnight_txs.len(),
            "work: load — parsing transactions"
        );

        // Parse extracted midnight txs into structured transactions
        let parsed = self.parser.parse_block(&substrate_block).map_err(|e| {
            DomainError::ChainError(dolos_core::ChainError::ChainSpecific(e))
        })?;

        debug!(
            slot,
            parsed_txs = parsed.transactions.len(),
            "work: load — parsing complete"
        );

        self.parsed = Some(parsed);
        Ok(())
    }

    fn compute(&mut self) -> Result<(), DomainError<MidnightError>> {
        let parsed = match self.parsed.take() {
            Some(p) => p,
            None => {
                debug!(slot = self.block.number, "work: compute — no parsed data, skipping");
                return Ok(());
            }
        };

        let slot = parsed.slot;
        debug!(slot, txs = parsed.transactions.len(), "work: compute — building deltas");

        for tx in &parsed.transactions {
            // Shielded outputs → CreateShieldedUtxo deltas
            for output in &tx.shielded_outputs {
                self.deltas.push(MidnightDelta::CreateShieldedUtxo(Box::new(
                    CreateShieldedUtxo {
                        state: ShieldedUtxoState {
                            intent_hash: tx.tx_hash,
                            output_index: output.output_index,
                            ciphertexts: output.ciphertexts.clone(),
                            created_slot: slot,
                            created_tx_hash: tx.tx_hash,
                            spent_slot: None,
                            spent_tx_hash: None,
                            ledger_version: output.ledger_version,
                            raw_coin: output.raw_coin.clone(),
                        },
                    },
                )));
            }

            // Unshielded outputs → CreateUnshieldedUtxo deltas
            for output in &tx.unshielded_outputs {
                self.deltas
                    .push(MidnightDelta::CreateUnshieldedUtxo(Box::new(
                        CreateUnshieldedUtxo {
                            state: UnshieldedUtxoState {
                                owner: output.owner,
                                token_type: output.token_type,
                                value: output.value,
                                intent_hash: tx.tx_hash,
                                output_index: output.output_index,
                                created_slot: slot,
                                created_tx_hash: tx.tx_hash,
                                spent_slot: None,
                                spent_tx_hash: None,
                            },
                        },
                    )));
            }

            // Shielded inputs → SpendShieldedUtxo deltas
            // Note: Matching nullifiers to UTxO keys requires a nullifier index
            // (mapping nullifier → entity key). For the PoC, we skip spend tracking.
            // A real implementation would look up the nullifier in the state store.
            if !tx.shielded_inputs.is_empty() {
                debug!(
                    slot,
                    count = tx.shielded_inputs.len(),
                    "skipping shielded spend tracking (nullifier index not implemented)"
                );
            }
        }

        if !self.deltas.is_empty() {
            info!(
                slot,
                deltas = self.deltas.len(),
                "work: compute — produced entity deltas"
            );
        }

        Ok(())
    }

    fn commit_wal<D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>>(
        &mut self,
        domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        let point = self.block.point();
        debug!(slot = self.block.number, "work: commit_wal");

        let log_value = LogValue {
            block: (*self.block.raw_bytes).clone(),
            delta: self.deltas.clone(),
            inputs: Default::default(),
        };

        domain.wal().append_entries(vec![(point, log_value)])?;

        Ok(())
    }

    fn commit_state<D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>>(
        &mut self,
        domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        let point = self.block.point();
        debug!(slot = self.block.number, entities = self.deltas.len(), "work: commit_state");
        let writer = domain.state().start_writer()?;

        // Apply entity deltas
        for delta in &mut self.deltas {
            let ns_key = dolos_core::EntityDelta::key(delta);
            let existing = domain
                .state()
                .read_entity_typed::<model::MidnightEntity>(ns_key.0, &ns_key.1)?;
            let mut entity = existing;
            dolos_core::EntityDelta::apply(delta, &mut entity);

            if let Some(ref ent) = entity {
                let (ns, value) = model::MidnightEntity::encode_entity(ent);
                writer.write_entity(ns, &ns_key.1, &value)?;
            }
        }

        writer.set_cursor(point)?;
        writer.commit()?;

        Ok(())
    }

    fn commit_archive<
        D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>,
    >(
        &mut self,
        domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        let point = self.block.point();
        let raw = &self.block.raw_bytes;
        debug!(slot = self.block.number, raw_bytes = raw.len(), "work: commit_archive");

        let writer = domain.archive().start_writer()?;
        writer.apply(&point, raw)?;
        writer.commit()?;

        Ok(())
    }

    fn tip_events(&self) -> Vec<TipEvent> {
        vec![TipEvent::Apply(
            self.block.point(),
            self.block.raw_bytes.clone(),
        )]
    }
}
