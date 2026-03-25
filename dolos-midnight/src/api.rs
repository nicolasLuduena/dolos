use std::sync::Arc;

use axum::{extract::State, routing, Json, Router};
use serde::{Deserialize, Serialize};

use dolos_core::StateStore;

use crate::decrypt::UtxoDecryptor;
use crate::domain::MidnightDomain;

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

// ---------------------------------------------------------------------------
// Handlers (stubs — Front 3 fills these in)
// ---------------------------------------------------------------------------

async fn query_shielded_utxos<D: UtxoDecryptor>(
    State(state): State<Arc<ApiState<D>>>,
    Json(_req): Json<ShieldedUtxoRequest>,
) -> Json<ShieldedUtxoResponse> {
    // Front 3 will: iterate NS_SHIELDED_UTXO entities, trial-decrypt with
    // the viewing key, filter by slot range and spent status.
    let tip_slot = state
        .domain
        .state
        .read_cursor()
        .ok()
        .flatten()
        .map(|p| match p {
            dolos_core::ChainPoint::Specific(slot, _) => slot,
            _ => 0,
        });

    Json(ShieldedUtxoResponse {
        utxos: Vec::new(),
        tip_slot,
    })
}

async fn query_unshielded_utxos<D: UtxoDecryptor>(
    State(state): State<Arc<ApiState<D>>>,
    axum::extract::Query(_query): axum::extract::Query<UnshieldedQuery>,
) -> Json<UnshieldedUtxoResponse> {
    // Front 3 will: iterate NS_UNSHIELDED_UTXO entities, filter by owner.
    let tip_slot = state
        .domain
        .state
        .read_cursor()
        .ok()
        .flatten()
        .map(|p| match p {
            dolos_core::ChainPoint::Specific(slot, _) => slot,
            _ => 0,
        });

    Json(UnshieldedUtxoResponse {
        utxos: Vec::new(),
        tip_slot,
    })
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

pub fn router<D: UtxoDecryptor + 'static>(
    domain: MidnightDomain,
    decryptor: D,
) -> Router {
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
