use gasket::framework::*;
use pallas::ledger::configs::byron;
use pallas::ledger::traverse::MultiEraBlock;
use tracing::info;

use crate::prelude::*;

pub type UpstreamPort = gasket::messaging::tokio::InputPort<RollEvent>;

#[derive(Stage)]
#[stage(name = "ledger", unit = "RollEvent", worker = "Worker")]
pub struct Stage {
    ledger: crate::ledger::store::LedgerStore,
    byron: byron::GenesisFile,

    pub upstream: UpstreamPort,

    #[metric]
    block_count: gasket::metrics::Counter,

    #[metric]
    wal_count: gasket::metrics::Counter,
}

impl Stage {
    pub fn new(ledger: crate::ledger::store::LedgerStore, byron: byron::GenesisFile) -> Self {
        Self {
            ledger,
            byron,
            upstream: Default::default(),
            block_count: Default::default(),
            wal_count: Default::default(),
        }
    }
}

pub struct Worker;

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(_stage: &Stage) -> Result<Self, WorkerError> {
        Ok(Self)
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<RollEvent>, WorkerError> {
        let msg = stage.upstream.recv().await.or_panic()?;

        Ok(WorkSchedule::Unit(msg.payload))
    }

    async fn execute(&mut self, unit: &RollEvent, stage: &mut Stage) -> Result<(), WorkerError> {
        match unit {
            RollEvent::Apply(slot, _, cbor) => {
                info!(slot, "applying block");

                let block = MultiEraBlock::decode(cbor).or_panic()?;

                let delta = crate::ledger::compute_delta(&block);
                stage.ledger.apply(&[delta]).or_panic()?;
            }
            RollEvent::Undo(slot, _, cbor) => {
                info!(slot, "undoing block");

                let block = MultiEraBlock::decode(cbor).or_panic()?;

                let delta = crate::ledger::compute_undo_delta(&block);
                stage.ledger.apply(&[delta]).or_panic()?;
            }
            RollEvent::Origin => {
                info!("applying origin");

                let delta = crate::ledger::compute_origin_delta(&stage.byron);
                stage.ledger.apply(&[delta]).or_panic()?;
            }
        };

        Ok(())
    }
}
