use dolos_core::{
    Block, ChainPoint, Domain, DomainError, LogValue, TipEvent, WorkUnit,
    ArchiveStore as _, ArchiveWriter as _, StateStore, StateWriter as _,
    WalStore,
};

use crate::chain::{MidnightBlock, MidnightError};
use crate::model::MidnightDelta;

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
        // Index updates will be implemented in Front 1
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
    deltas: Vec<MidnightDelta>,
}

impl RollWorkUnit {
    pub fn new(block: MidnightBlock) -> Self {
        Self {
            block,
            deltas: Vec::new(),
        }
    }

    fn load<D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>>(
        &mut self,
        _domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        // Front 1 will: query state store for UTxOs referenced as inputs,
        // parse extrinsics via ExtrinsicParser, build deltas.
        // For Phase 0, this is a no-op.
        Ok(())
    }

    fn compute(&mut self) -> Result<(), DomainError<MidnightError>> {
        // Front 1 will: build MidnightDelta + UtxoSetDelta + IndexDelta
        // from the parsed block. For Phase 0, no-op.
        Ok(())
    }

    fn commit_wal<D: Domain<ChainSpecificError = MidnightError, EntityDelta = MidnightDelta>>(
        &mut self,
        domain: &D,
    ) -> Result<(), DomainError<MidnightError>> {
        let point = self.block.point();

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
        let writer = domain.state().start_writer()?;

        // Front 1 will: load entities, apply deltas, write back.
        // For Phase 0, just advance the cursor.
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
