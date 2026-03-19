pub mod runtime;
pub mod api;

use dolos_core::{
    archive::{ArchiveWriter},
    config::{StorageConfig, SyncConfig},
    state::{Entity, EntityDelta, EntityKey, Namespace, StateStore, StateWriter},
    work_unit::WorkUnit,
    BlockSlot, ChainError, ChainLogic, ChainPoint, Domain, DomainError, EraCbor, IndexDelta,
    TipEvent, TxoRef,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use subxt::{OnlineClient, SubstrateConfig};
use midnight_serialize::{tagged_deserialize};
use midnight_ledger_v8::structure::{Transaction as LedgerTransactionV8, ProofMarker as ProofMarkerV8};
use midnight_base_crypto::signatures::Signature as SignatureV7;
use midnight_transient_crypto::commitment::PureGeneratorPedersen;
use midnight_storage_core::db::InMemoryDB;
use crate::runtime::midnight_runtime::Call;
use crate::runtime::midnight_runtime::runtime_types::pallet_midnight::pallet::Call::send_mn_transaction;
use std::collections::{VecDeque};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Subxt error: {0}")]
    Subxt(#[from] subxt::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Infallible")]
    Infallible(#[from] std::convert::Infallible),
    #[error("Connection timeout")]
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightConfig {
    pub rpc_url: String,
    pub data_dir: String,
}

impl Default for MidnightConfig {
    fn default() -> Self {
        Self {
            rpc_url: "wss://rpc.preprod.midnight.network".to_string(),
            data_dir: "./data/midnight".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightBlock {
    pub number: u64,
    pub hash: [u8; 32],
    pub raw: Arc<Vec<u8>>,
}

impl dolos_core::Block for MidnightBlock {
    fn slot(&self) -> u64 {
        self.number
    }
    fn hash(&self) -> dolos_core::hash::Hash<32> {
        dolos_core::hash::Hash::new(self.hash)
    }
    fn depends_on(
        &self,
        _inputs: &mut std::collections::HashMap<TxoRef, Arc<EraCbor>>,
    ) -> Vec<TxoRef> {
        vec![]
    }
    fn raw(&self) -> Arc<Vec<u8>> {
        self.raw.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnshieldedUtxo {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
    pub amount: u128,
    pub owner: [u8; 32],
}

pub const UTXO_NAMESPACE: Namespace = "unshielded_utxo";
pub const COMMITMENT_NAMESPACE: Namespace = "zswap_commitment";
pub const NULLIFIER_NAMESPACE: Namespace = "zswap_nullifier";

impl Entity for UnshieldedUtxo {
    type ChainSpecificError = Error;

    fn decode_entity(
        _ns: Namespace,
        value: &Vec<u8>,
    ) -> Result<Self, ChainError<Self::ChainSpecificError>> {
        bincode::deserialize(value)
            .map_err(|e| Error::Serialization(e.to_string()))
            .map_err(ChainError::ChainSpecific)
    }

    fn encode_entity(value: &Self) -> (Namespace, Vec<u8>) {
        let bytes = bincode::serialize(value).expect("serialization failed");
        (UTXO_NAMESPACE, bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZswapCommitment {
    pub hash: [u8; 32],
}

impl Entity for ZswapCommitment {
    type ChainSpecificError = Error;

    fn decode_entity(
        _ns: Namespace,
        value: &Vec<u8>,
    ) -> Result<Self, ChainError<Self::ChainSpecificError>> {
        bincode::deserialize(value)
            .map_err(|e| Error::Serialization(e.to_string()))
            .map_err(ChainError::ChainSpecific)
    }

    fn encode_entity(value: &Self) -> (Namespace, Vec<u8>) {
        let bytes = bincode::serialize(value).expect("serialization failed");
        (COMMITMENT_NAMESPACE, bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZswapNullifier {
    pub hash: [u8; 32],
}

impl Entity for ZswapNullifier {
    type ChainSpecificError = Error;

    fn decode_entity(
        _ns: Namespace,
        value: &Vec<u8>,
    ) -> Result<Self, ChainError<Self::ChainSpecificError>> {
        bincode::deserialize(value)
            .map_err(|e| Error::Serialization(e.to_string()))
            .map_err(ChainError::ChainSpecific)
    }

    fn encode_entity(value: &Self) -> (Namespace, Vec<u8>) {
        let bytes = bincode::serialize(value).expect("serialization failed");
        (NULLIFIER_NAMESPACE, bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MidnightDeltaValue {
    UnshieldedUtxo(Option<UnshieldedUtxo>),
    ZswapCommitment(Option<ZswapCommitment>),
    ZswapNullifier(Option<ZswapNullifier>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidnightDelta {
    pub key: EntityKey,
    pub new_value: MidnightDeltaValue,
}

impl EntityDelta for MidnightDelta {
    type Entity = UnshieldedUtxo;

    fn key(&self) -> dolos_core::state::NsKey {
        let ns = match &self.new_value {
            MidnightDeltaValue::UnshieldedUtxo(_) => UTXO_NAMESPACE,
            MidnightDeltaValue::ZswapCommitment(_) => COMMITMENT_NAMESPACE,
            MidnightDeltaValue::ZswapNullifier(_) => NULLIFIER_NAMESPACE,
        };
        dolos_core::state::NsKey::from((ns, self.key.clone()))
    }

    fn apply(&mut self, _entity: &mut Option<Self::Entity>) {
        // Not implemented due to trait limitations
    }

    fn undo(&self, _entity: &mut Option<Self::Entity>) {
        todo!()
    }
}

#[derive(Clone)]
pub struct MidnightDomain {
    pub storage_config: Arc<StorageConfig>,
    pub sync_config: Arc<SyncConfig>,
    pub state: dolos_fjall::StateStore,
    pub archive: dolos_redb3::archive::ArchiveStore<Error>,
    pub indexes: dolos_fjall::IndexStore,
    pub mempool: dolos_core::builtin::EphemeralMempool,
    pub wal: dolos_redb3::wal::RedbWalStore<MidnightDelta>,
    pub chain: Arc<std::sync::RwLock<MidnightLogic>>,
    pub genesis: Arc<MidnightGenesis>,
    pub subxt: Arc<OnlineClient<SubstrateConfig>>,
}

pub struct MidnightTipSubscription {
    pub receiver: tokio::sync::broadcast::Receiver<TipEvent>,
}

impl dolos_core::TipSubscription for MidnightTipSubscription {
    #[allow(refining_impl_trait)]
    fn next_tip(&mut self) -> impl std::future::Future<Output = TipEvent> + Send {
        let mut receiver = self.receiver.resubscribe();
        async move { receiver.recv().await.unwrap() }
    }
}

impl Domain for MidnightDomain {
    type Entity = UnshieldedUtxo;
    type EntityDelta = MidnightDelta;
    type Genesis = MidnightGenesis;
    type ChainSpecificError = Error;

    type Chain = MidnightLogic;
    type WorkUnit = MidnightWorkUnit<Self>;

    type Wal = dolos_redb3::wal::RedbWalStore<MidnightDelta>;
    type State = dolos_fjall::StateStore;
    type Archive = dolos_redb3::archive::ArchiveStore<Error>;
    type Indexes = dolos_fjall::IndexStore;
    type Mempool = dolos_core::builtin::EphemeralMempool;
    type TipSubscription = MidnightTipSubscription;

    fn storage_config(&self) -> &StorageConfig {
        &self.storage_config
    }
    fn sync_config(&self) -> &SyncConfig {
        &self.sync_config
    }
    fn genesis(&self) -> Arc<Self::Genesis> {
        self.genesis.clone()
    }
    fn read_chain(&self) -> std::sync::RwLockReadGuard<'_, Self::Chain> {
        self.chain.read().unwrap()
    }
    fn write_chain(&self) -> std::sync::RwLockWriteGuard<'_, Self::Chain> {
        self.chain.write().unwrap()
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
    ) -> Result<Self::TipSubscription, DomainError<Self::ChainSpecificError>> {
        todo!()
    }
    fn notify_tip(&self, _tip: TipEvent) {
        todo!()
    }
}

#[derive(Clone, Debug)]
pub struct MidnightGenesis {
    pub chain_id: u32,
    pub genesis_hash: [u8; 32],
}

impl dolos_core::Genesis for MidnightGenesis {
    fn chain_id(&self) -> u32 {
        self.chain_id
    }
}

pub struct MidnightWorkUnit<D: Domain> {
    pub block: subxt::blocks::Block<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    pub raw: Arc<Vec<u8>>,
    pub deltas: Vec<MidnightDelta>,
    pub _phantom: std::marker::PhantomData<D>,
}

impl<D: Domain<ChainSpecificError = Error, EntityDelta = MidnightDelta>> WorkUnit<D> for MidnightWorkUnit<D> {
    fn name(&self) -> &'static str {
        "midnight-work-unit"
    }
    fn load(&mut self, _domain: &D) -> Result<(), DomainError<Error>> {
        Ok(())
    }
    fn compute(&mut self) -> Result<(), DomainError<Error>> {
        // satisfaction of trait
        Ok(())
    }
    fn commit_state(&mut self, _domain: &D) -> Result<(), DomainError<Error>> {
        Ok(())
    }
    fn commit_archive(&mut self, _domain: &D) -> Result<(), DomainError<Error>> {
        Ok(())
    }
}

impl MidnightWorkUnit<MidnightDomain> {
    pub async fn compute_async(&mut self) -> Result<(), DomainError<Error>> {
        let extrinsics = self.block.extrinsics().await.map_err(|e| DomainError::ChainError(ChainError::ChainSpecific(Error::Subxt(e))))?;

        for ext in extrinsics.iter() {
            if let Ok(call) = ext.as_root_extrinsic::<Call>() {
                match call {
                    Call::Midnight(send_mn_transaction { midnight_tx }) => {
                        let mut data = &midnight_tx[..];
                        if let Ok(tx) = tagged_deserialize::<MidnightTransactionV8>(&mut data) {
                            match tx {
                                LedgerTransactionV8::Standard(std_tx) => {
                                    for (segment_id, intent) in std_tx.intents {
                                        let ledger_intent_hash = intent.erase_proofs().erase_signatures().intent_hash(segment_id);
                                        let intent_hash_bytes = ledger_intent_hash.0.0;

                                        for (idx, output) in intent.guaranteed_outputs().into_iter().enumerate() {
                                            let utxo = UnshieldedUtxo {
                                                intent_hash: intent_hash_bytes,
                                                output_index: idx as u32,
                                                amount: output.value,
                                                owner: output.owner.0.0,
                                            };
                                            
                                            let mut key_bytes = [0u8; 32];
                                            key_bytes[0..28].copy_from_slice(&intent_hash_bytes[0..28]);
                                            key_bytes[28..32].copy_from_slice(&(idx as u32).to_be_bytes());

                                            self.deltas.push(MidnightDelta {
                                                key: EntityKey::from(&key_bytes),
                                                new_value: MidnightDeltaValue::UnshieldedUtxo(Some(utxo)),
                                            });
                                        }
                                    }

                                    // Extract from guaranteed and fallible shielded coins
                                    if let Some(gc) = std_tx.guaranteed_coins {
                                        for (out_idx, output) in gc.outputs.iter().enumerate() {
                                            let commitment = ZswapCommitment {
                                                hash: output.coin_com.0.0,
                                            };
                                            let mut key_bytes = [0u8; 32];
                                            let block_hash_bytes = self.block.hash().0;
                                            key_bytes[0..28].copy_from_slice(&block_hash_bytes[0..28]);
                                            key_bytes[28..30].copy_from_slice(&(0u16).to_be_bytes());
                                            key_bytes[30..32].copy_from_slice(&(out_idx as u16).to_be_bytes());

                                            self.deltas.push(MidnightDelta {
                                                key: EntityKey::from(&key_bytes),
                                                new_value: MidnightDeltaValue::ZswapCommitment(Some(commitment)),
                                            });
                                        }
                                        for (in_idx, input) in gc.inputs.iter().enumerate() {
                                            let nullifier = ZswapNullifier {
                                                hash: input.nullifier.0.0,
                                            };
                                            let mut key_bytes = [0u8; 32];
                                            let block_hash_bytes = self.block.hash().0;
                                            key_bytes[0..28].copy_from_slice(&block_hash_bytes[0..28]);
                                            key_bytes[28..30].copy_from_slice(&(0u16).to_be_bytes());
                                            key_bytes[30..32].copy_from_slice(&(in_idx as u16).to_be_bytes());

                                            self.deltas.push(MidnightDelta {
                                                key: EntityKey::from(&key_bytes),
                                                new_value: MidnightDeltaValue::ZswapNullifier(Some(nullifier)),
                                            });
                                        }
                                    }

                                    for (idx, (_, fc)) in std_tx.fallible_coins.into_iter().enumerate() {
                                        let base_idx = (idx + 1) as u16; // offset from guaranteed
                                        for (out_idx, output) in fc.outputs.iter().enumerate() {
                                            let commitment = ZswapCommitment {
                                                hash: output.coin_com.0.0,
                                            };
                                            let mut key_bytes = [0u8; 32];
                                            let block_hash_bytes = self.block.hash().0;
                                            key_bytes[0..28].copy_from_slice(&block_hash_bytes[0..28]);
                                            key_bytes[28..30].copy_from_slice(&(base_idx).to_be_bytes());
                                            key_bytes[30..32].copy_from_slice(&(out_idx as u16).to_be_bytes());

                                            self.deltas.push(MidnightDelta {
                                                key: EntityKey::from(&key_bytes),
                                                new_value: MidnightDeltaValue::ZswapCommitment(Some(commitment)),
                                            });
                                        }
                                        for (in_idx, input) in fc.inputs.iter().enumerate() {
                                            let nullifier = ZswapNullifier {
                                                hash: input.nullifier.0.0,
                                            };
                                            let mut key_bytes = [0u8; 32];
                                            let block_hash_bytes = self.block.hash().0;
                                            key_bytes[0..28].copy_from_slice(&block_hash_bytes[0..28]);
                                            key_bytes[28..30].copy_from_slice(&(base_idx).to_be_bytes());
                                            key_bytes[30..32].copy_from_slice(&(in_idx as u16).to_be_bytes());

                                            self.deltas.push(MidnightDelta {
                                                key: EntityKey::from(&key_bytes),
                                                new_value: MidnightDeltaValue::ZswapNullifier(Some(nullifier)),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn commit_state_with_domain(&mut self, domain: &MidnightDomain) -> Result<(), DomainError<Error>> {
        let writer = domain.state().start_writer().map_err(DomainError::StateError)?;
        for delta in &self.deltas {
            match &delta.new_value {
                MidnightDeltaValue::UnshieldedUtxo(u) => {
                    writer.save_entity_typed(UTXO_NAMESPACE, &delta.key, u.as_ref())
                        .map_err(DomainError::StateError)?;
                }
                MidnightDeltaValue::ZswapCommitment(c) => {
                    writer.save_entity_typed(COMMITMENT_NAMESPACE, &delta.key, c.as_ref())
                        .map_err(DomainError::StateError)?;
                }
                MidnightDeltaValue::ZswapNullifier(n) => {
                    writer.save_entity_typed(NULLIFIER_NAMESPACE, &delta.key, n.as_ref())
                        .map_err(DomainError::StateError)?;
                }
            }
        }
        writer.commit().map_err(DomainError::StateError)?;
        Ok(())
    }

    fn commit_archive_with_domain(&mut self, domain: &MidnightDomain) -> Result<(), DomainError<Error>> {
        let writer = domain.archive().start_writer().map_err(|e| DomainError::ArchiveError(dolos_core::archive::ArchiveError::InternalError(e.to_string())))?;
        let point = ChainPoint::Specific(self.block.number() as u64, dolos_core::hash::Hash::new(self.block.hash().0));
        
        writer.apply(&point, &self.raw).map_err(|e| DomainError::ArchiveError(dolos_core::archive::ArchiveError::InternalError(e.to_string())))?;
        writer.commit().map_err(|e| DomainError::ArchiveError(dolos_core::archive::ArchiveError::InternalError(e.to_string())))?;
        Ok(())
    }
}

pub struct MidnightLogic {
    pub queue: VecDeque<subxt::blocks::Block<SubstrateConfig, OnlineClient<SubstrateConfig>>>,
}

impl ChainLogic for MidnightLogic {
    type Config = ();
    type Block = MidnightBlock;
    type Entity = UnshieldedUtxo;
    type Utxo = UnshieldedUtxo;
    type Delta = MidnightDelta;
    type Genesis = MidnightGenesis;
    type ChainSpecificError = Error;

    type WorkUnit<D: Domain<Chain = Self, Entity = Self::Entity, EntityDelta = Self::Delta, ChainSpecificError = Self::ChainSpecificError>> = MidnightWorkUnit<D>;

    fn initialize<D: Domain>(
        _config: Self::Config,
        _state: &D::State,
        _genesis: Self::Genesis,
    ) -> Result<Self, ChainError<Self::ChainSpecificError>> {
        Ok(Self { queue: VecDeque::new() })
    }

    fn can_receive_block(&self) -> bool {
        self.queue.len() < 100
    }

    fn receive_block(
        &mut self,
        _raw: dolos_core::RawBlock,
    ) -> Result<BlockSlot, ChainError<Self::ChainSpecificError>> {
        Ok(0)
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
        None
    }

    fn compute_undo(
        _block: &dolos_core::Cbor,
        _inputs: &std::collections::HashMap<TxoRef, Arc<EraCbor>>,
        _point: ChainPoint,
    ) -> Result<dolos_core::UndoBlockData, ChainError<Self::ChainSpecificError>> {
        Ok(dolos_core::UndoBlockData {
            utxo_delta: dolos_core::UtxoSetDelta::default(),
            index_delta: IndexDelta::default(),
            tx_hashes: vec![],
        })
    }

    fn decode_utxo(&self, _utxo: Arc<EraCbor>) -> Result<Self::Utxo, ChainError<Self::ChainSpecificError>> {
        todo!()
    }

    fn mutable_slots(_domain: &impl Domain<Genesis = Self::Genesis>) -> BlockSlot {
        0
    }

    fn tx_produced_utxos(_era_body: &EraCbor) -> Vec<(TxoRef, EraCbor)> {
        vec![]
    }

    fn tx_consumed_ref(_era_body: &EraCbor) -> Vec<TxoRef> {
        vec![]
    }

    fn find_tx_in_block(
        _block: &[u8],
        _tx_hash: &[u8],
    ) -> Result<Option<(EraCbor, dolos_core::TxOrder)>, Self::ChainSpecificError> {
        Ok(None)
    }

    fn validate_tx<D: Domain<ChainSpecificError = Self::ChainSpecificError>>(
        &self,
        _cbor: &[u8],
        _utxos: &dolos_core::mempool::MempoolAwareUtxoStore<D>,
        _tip: Option<ChainPoint>,
        _genesis: &Self::Genesis,
    ) -> Result<dolos_core::mempool::MempoolTx, ChainError<Self::ChainSpecificError>> {
        todo!()
    }
}

pub type MidnightTransactionV8 = LedgerTransactionV8<
    SignatureV7,
    ProofMarkerV8,
    PureGeneratorPedersen,
    InMemoryDB,
>;

pub async fn start_sync(config: MidnightConfig, domain: MidnightDomain) -> Result<(), Error> {
    tracing::error!("Attempting to connect to Midnight RPC: {}", config.rpc_url);
    
    let client = domain.subxt.clone();

    tracing::error!("Successfully connected to Midnight RPC!");
    
    tracing::error!("Subscribing to finalized blocks...");
    let mut finalized_blocks = client.blocks().subscribe_finalized().await.map_err(Error::Subxt)?;
    tracing::error!("Subscription successful, waiting for blocks...");

    while let Some(block_result) = finalized_blocks.next().await {
        let block = block_result.map_err(Error::Subxt)?;
        tracing::error!("BLOCK RECEIVED: #{} ({})", block.number(), block.hash());

        let mut raw_block_payload = Vec::new();
        if let Ok(extrinsics) = block.extrinsics().await {
            for ext in extrinsics.iter() {
                raw_block_payload.extend_from_slice(ext.bytes());
            }
        }

        let mut work = MidnightWorkUnit {
            block,
            raw: Arc::new(raw_block_payload),
            deltas: Vec::new(),
            _phantom: std::marker::PhantomData,
        };

        if let Err(e) = work.compute_async().await {
            tracing::error!("Failed to compute block: {:?}", e);
            continue;
        }

        if let Err(e) = work.commit_state_with_domain(&domain) {
            tracing::error!("Failed to commit state: {:?}", e);
        }
        if let Err(e) = work.commit_archive_with_domain(&domain) {
            tracing::error!("Failed to commit archive: {:?}", e);
        }

        tracing::error!("Successfully indexed block #{}", work.block.number());
        for delta in &work.deltas {
            match &delta.new_value {
                MidnightDeltaValue::UnshieldedUtxo(Some(utxo)) => {
                    tracing::error!("  Indexed Unshielded UTXO: owner={}, amount={}", hex::encode(utxo.owner), utxo.amount);
                }
                MidnightDeltaValue::ZswapCommitment(Some(c)) => {
                    tracing::error!("  Indexed Zswap Commitment: hash={}", hex::encode(c.hash));
                }
                MidnightDeltaValue::ZswapNullifier(Some(n)) => {
                    tracing::error!("  Indexed Zswap Nullifier: hash={}", hex::encode(n.hash));
                }
                _ => {}
            }
        }
    }

    Ok(())
}
