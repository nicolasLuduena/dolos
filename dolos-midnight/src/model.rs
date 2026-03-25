use dolos_core::state::{EntityKey, EntityValue, Namespace, NamespaceType, NsKey, StateSchema};
use dolos_core::{BlockSlot, ChainError, Entity, EntityDelta};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::chain::MidnightError;

pub const NS_SHIELDED_UTXO: Namespace = "mn_shielded_utxo";
pub const NS_UNSHIELDED_UTXO: Namespace = "mn_unshielded_utxo";

pub fn build_schema() -> StateSchema {
    let mut schema = StateSchema::default();
    schema.insert(NS_SHIELDED_UTXO, NamespaceType::KeyValue);
    schema.insert(NS_UNSHIELDED_UTXO, NamespaceType::KeyValue);
    schema
}

/// Compute a deterministic 32-byte entity key from intent_hash + output_index.
pub fn utxo_entity_key(intent_hash: &[u8; 32], output_index: u32) -> EntityKey {
    let mut hasher = Sha256::new();
    hasher.update(intent_hash);
    hasher.update(output_index.to_be_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    EntityKey::from(&key)
}

// ---------------------------------------------------------------------------
// Entity types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldedUtxoState {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
    /// Raw ciphertext blobs for trial decryption with viewing keys.
    pub ciphertexts: Vec<Vec<u8>>,
    pub created_slot: BlockSlot,
    pub created_tx_hash: [u8; 32],
    pub spent_slot: Option<BlockSlot>,
    pub spent_tx_hash: Option<[u8; 32]>,
    /// Ledger version tag (e.g. 7 or 8) for deserialize dispatch.
    pub ledger_version: u16,
    /// The raw serialized coin data (for full decode if needed).
    pub raw_coin: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnshieldedUtxoState {
    pub owner: [u8; 32],
    pub token_type: [u8; 32],
    pub value: u128,
    pub intent_hash: [u8; 32],
    pub output_index: u32,
    pub created_slot: BlockSlot,
    pub created_tx_hash: [u8; 32],
    pub spent_slot: Option<BlockSlot>,
    pub spent_tx_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MidnightEntity {
    ShieldedUtxo(Box<ShieldedUtxoState>),
    UnshieldedUtxo(Box<UnshieldedUtxoState>),
}

impl Entity for MidnightEntity {
    type ChainSpecificError = MidnightError;

    fn decode_entity(
        ns: Namespace,
        value: &EntityValue,
    ) -> Result<Self, ChainError<MidnightError>> {
        match ns {
            NS_SHIELDED_UTXO => {
                let state: ShieldedUtxoState =
                    bincode::deserialize(value).map_err(|e| {
                        ChainError::ChainSpecific(MidnightError::Decode(e.to_string()))
                    })?;
                Ok(MidnightEntity::ShieldedUtxo(Box::new(state)))
            }
            NS_UNSHIELDED_UTXO => {
                let state: UnshieldedUtxoState =
                    bincode::deserialize(value).map_err(|e| {
                        ChainError::ChainSpecific(MidnightError::Decode(e.to_string()))
                    })?;
                Ok(MidnightEntity::UnshieldedUtxo(Box::new(state)))
            }
            other => Err(ChainError::InvalidNamespace(other)),
        }
    }

    fn encode_entity(value: &Self) -> (Namespace, EntityValue) {
        match value {
            MidnightEntity::ShieldedUtxo(state) => (
                NS_SHIELDED_UTXO,
                bincode::serialize(state.as_ref()).expect("bincode serialize"),
            ),
            MidnightEntity::UnshieldedUtxo(state) => (
                NS_UNSHIELDED_UTXO,
                bincode::serialize(state.as_ref()).expect("bincode serialize"),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Delta types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShieldedUtxo {
    pub state: ShieldedUtxoState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendShieldedUtxo {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
    pub spent_slot: BlockSlot,
    pub spent_tx_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnshieldedUtxo {
    pub state: UnshieldedUtxoState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendUnshieldedUtxo {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
    pub spent_slot: BlockSlot,
    pub spent_tx_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MidnightDelta {
    CreateShieldedUtxo(Box<CreateShieldedUtxo>),
    SpendShieldedUtxo(Box<SpendShieldedUtxo>),
    CreateUnshieldedUtxo(Box<CreateUnshieldedUtxo>),
    SpendUnshieldedUtxo(Box<SpendUnshieldedUtxo>),
}

impl EntityDelta for MidnightDelta {
    type Entity = MidnightEntity;

    fn key(&self) -> NsKey {
        match self {
            MidnightDelta::CreateShieldedUtxo(d) => NsKey(
                NS_SHIELDED_UTXO,
                utxo_entity_key(&d.state.intent_hash, d.state.output_index),
            ),
            MidnightDelta::SpendShieldedUtxo(d) => NsKey(
                NS_SHIELDED_UTXO,
                utxo_entity_key(&d.intent_hash, d.output_index),
            ),
            MidnightDelta::CreateUnshieldedUtxo(d) => NsKey(
                NS_UNSHIELDED_UTXO,
                utxo_entity_key(&d.state.intent_hash, d.state.output_index),
            ),
            MidnightDelta::SpendUnshieldedUtxo(d) => NsKey(
                NS_UNSHIELDED_UTXO,
                utxo_entity_key(&d.intent_hash, d.output_index),
            ),
        }
    }

    fn apply(&mut self, entity: &mut Option<Self::Entity>) {
        match self {
            MidnightDelta::CreateShieldedUtxo(d) => {
                *entity = Some(MidnightEntity::ShieldedUtxo(Box::new(d.state.clone())));
            }
            MidnightDelta::SpendShieldedUtxo(d) => {
                if let Some(MidnightEntity::ShieldedUtxo(ref mut state)) = entity {
                    state.spent_slot = Some(d.spent_slot);
                    state.spent_tx_hash = Some(d.spent_tx_hash);
                }
            }
            MidnightDelta::CreateUnshieldedUtxo(d) => {
                *entity = Some(MidnightEntity::UnshieldedUtxo(Box::new(d.state.clone())));
            }
            MidnightDelta::SpendUnshieldedUtxo(d) => {
                if let Some(MidnightEntity::UnshieldedUtxo(ref mut state)) = entity {
                    state.spent_slot = Some(d.spent_slot);
                    state.spent_tx_hash = Some(d.spent_tx_hash);
                }
            }
        }
    }

    fn undo(&self, entity: &mut Option<Self::Entity>) {
        match self {
            MidnightDelta::CreateShieldedUtxo(_)
            | MidnightDelta::CreateUnshieldedUtxo(_) => {
                *entity = None;
            }
            MidnightDelta::SpendShieldedUtxo(_) => {
                if let Some(MidnightEntity::ShieldedUtxo(ref mut state)) = entity {
                    state.spent_slot = None;
                    state.spent_tx_hash = None;
                }
            }
            MidnightDelta::SpendUnshieldedUtxo(_) => {
                if let Some(MidnightEntity::UnshieldedUtxo(ref mut state)) = entity {
                    state.spent_slot = None;
                    state.spent_tx_hash = None;
                }
            }
        }
    }
}
