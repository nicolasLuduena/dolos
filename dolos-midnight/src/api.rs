use std::sync::Arc;

use axum::{extract::State, http::StatusCode, routing, Json, Router};
use serde::{Deserialize, Serialize};

use dolos_core::StateStore;

use crate::decrypt::UtxoDecryptor;
use crate::domain::MidnightDomain;
use crate::model::{MidnightEntity, NS_SHIELDED_UTXO, NS_UNSHIELDED_UTXO};

// ---------------------------------------------------------------------------
// Shared state for Axum handlers
// ---------------------------------------------------------------------------

pub struct ApiState<D: UtxoDecryptor> {
    pub domain: MidnightDomain,
    pub decryptor: D,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ShieldedUtxoRequest {
    /// Hex-encoded 32-byte viewing key.
    pub viewing_key: String,
    #[serde(default)]
    pub from_slot: Option<u64>,
    #[serde(default)]
    pub to_slot: Option<u64>,
    #[serde(default = "default_include_spent")]
    pub include_spent: bool,
}

fn default_include_spent() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct ShieldedUtxoResponse {
    pub utxos: Vec<ShieldedUtxoInfo>,
    pub tip_slot: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ShieldedUtxoInfo {
    pub intent_hash: String,
    pub output_index: u32,
    pub created_slot: u64,
    pub created_tx_hash: String,
    pub spent_slot: Option<u64>,
    pub spent_tx_hash: Option<String>,
    pub decrypted: Option<DecryptedInfo>,
}

#[derive(Debug, Serialize)]
pub struct DecryptedInfo {
    pub value: String,
    pub token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UnshieldedQuery {
    pub owner: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UnshieldedUtxoResponse {
    pub utxos: Vec<UnshieldedUtxoInfo>,
    pub tip_slot: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UnshieldedUtxoInfo {
    pub owner: String,
    pub token_type: String,
    pub value: String,
    pub intent_hash: String,
    pub output_index: u32,
    pub created_slot: u64,
    pub created_tx_hash: String,
    pub spent_slot: Option<u64>,
    pub spent_tx_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub tip_slot: Option<u64>,
    pub tip_hash: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_tip_slot(state: &MidnightDomain) -> Option<u64> {
    state
        .state
        .read_cursor()
        .ok()
        .flatten()
        .map(|p| p.slot())
}

fn parse_hex_32(hex_str: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str).map_err(|e| format!("invalid hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn query_shielded_utxos<D: UtxoDecryptor>(
    State(state): State<Arc<ApiState<D>>>,
    Json(req): Json<ShieldedUtxoRequest>,
) -> Result<Json<ShieldedUtxoResponse>, (StatusCode, Json<ApiError>)> {
    let viewing_key = parse_hex_32(&req.viewing_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError { error: e }),
        )
    })?;

    let tip_slot = read_tip_slot(&state.domain);

    let iter = state
        .domain
        .state
        .iter_entities_typed::<MidnightEntity>(NS_SHIELDED_UTXO, None)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("state iteration error: {e}"),
                }),
            )
        })?;

    let mut utxos = Vec::new();

    for entry in iter {
        let (_key, entity) = entry.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("entity decode error: {e}"),
                }),
            )
        })?;

        let utxo_state = match &entity {
            MidnightEntity::ShieldedUtxo(s) => s,
            _ => continue,
        };

        // Slot range filter
        if let Some(from) = req.from_slot {
            if utxo_state.created_slot < from {
                continue;
            }
        }
        if let Some(to) = req.to_slot {
            if utxo_state.created_slot > to {
                continue;
            }
        }

        // Spent filter
        if !req.include_spent && utxo_state.spent_slot.is_some() {
            continue;
        }

        // Trial decryption — only include UTxOs visible to this viewing key
        if !state.decryptor.can_decrypt(&viewing_key, utxo_state) {
            continue;
        }

        let decrypted = state
            .decryptor
            .decrypt_info(&viewing_key, utxo_state)
            .map(|info| DecryptedInfo {
                value: info.value.to_string(),
                token_type: info.token_type.map(hex::encode),
            });

        utxos.push(ShieldedUtxoInfo {
            intent_hash: hex::encode(utxo_state.intent_hash),
            output_index: utxo_state.output_index,
            created_slot: utxo_state.created_slot,
            created_tx_hash: hex::encode(utxo_state.created_tx_hash),
            spent_slot: utxo_state.spent_slot,
            spent_tx_hash: utxo_state.spent_tx_hash.map(hex::encode),
            decrypted,
        });
    }

    Ok(Json(ShieldedUtxoResponse { utxos, tip_slot }))
}

async fn query_unshielded_utxos<D: UtxoDecryptor>(
    State(state): State<Arc<ApiState<D>>>,
    axum::extract::Query(query): axum::extract::Query<UnshieldedQuery>,
) -> Result<Json<UnshieldedUtxoResponse>, (StatusCode, Json<ApiError>)> {
    let owner_filter = query
        .owner
        .as_deref()
        .map(parse_hex_32)
        .transpose()
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError { error: e }),
            )
        })?;

    let tip_slot = read_tip_slot(&state.domain);

    let iter = state
        .domain
        .state
        .iter_entities_typed::<MidnightEntity>(NS_UNSHIELDED_UTXO, None)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("state iteration error: {e}"),
                }),
            )
        })?;

    let mut utxos = Vec::new();

    for entry in iter {
        let (_key, entity) = entry.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("entity decode error: {e}"),
                }),
            )
        })?;

        let utxo_state = match &entity {
            MidnightEntity::UnshieldedUtxo(s) => s,
            _ => continue,
        };

        // Owner filter
        if let Some(ref owner) = owner_filter {
            if utxo_state.owner != *owner {
                continue;
            }
        }

        utxos.push(UnshieldedUtxoInfo {
            owner: hex::encode(utxo_state.owner),
            token_type: hex::encode(utxo_state.token_type),
            value: utxo_state.value.to_string(),
            intent_hash: hex::encode(utxo_state.intent_hash),
            output_index: utxo_state.output_index,
            created_slot: utxo_state.created_slot,
            created_tx_hash: hex::encode(utxo_state.created_tx_hash),
            spent_slot: utxo_state.spent_slot,
            spent_tx_hash: utxo_state.spent_tx_hash.map(hex::encode),
        });
    }

    Ok(Json(UnshieldedUtxoResponse { utxos, tip_slot }))
}

async fn get_status<D: UtxoDecryptor>(
    State(state): State<Arc<ApiState<D>>>,
) -> Json<StatusResponse> {
    let cursor = state.domain.state.read_cursor().ok().flatten();

    let (tip_slot, tip_hash) = match cursor {
        Some(dolos_core::ChainPoint::Specific(slot, hash)) => {
            (Some(slot), Some(hex::encode(hash.as_ref())))
        }
        _ => (None, None),
    };

    Json(StatusResponse { tip_slot, tip_hash })
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router<D: UtxoDecryptor + 'static>(domain: MidnightDomain, decryptor: D) -> Router {
    let state = Arc::new(ApiState { domain, decryptor });

    Router::new()
        .route(
            "/api/v1/utxos/shielded",
            routing::post(query_shielded_utxos::<D>),
        )
        .route(
            "/api/v1/utxos/unshielded",
            routing::get(query_unshielded_utxos::<D>),
        )
        .route("/api/v1/status", routing::get(get_status::<D>))
        .with_state(state)
}
