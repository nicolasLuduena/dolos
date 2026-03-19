use crate::{
    MidnightDomain, UnshieldedUtxo, ZswapCommitment, ZswapNullifier, COMMITMENT_NAMESPACE,
    NULLIFIER_NAMESPACE, UTXO_NAMESPACE,
};
use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use dolos_core::state::{EntityKey, StateStore};
use hex;
use serde::Serialize;

#[derive(Serialize)]
pub struct UtxoResponse {
    pub intent_hash: String,
    pub output_index: u32,
    pub amount: String, // Amount can be large, use String
    pub owner: String,
}

#[derive(Serialize)]
pub struct CommitmentResponse {
    pub hash: String,
}

#[derive(Serialize)]
pub struct NullifierResponse {
    pub hash: String,
}

#[derive(Serialize)]
pub struct BlockResponse {
    pub slot: u64,
    pub size: usize,
}

pub async fn serve_api(domain: MidnightDomain, port: u16) {
    let app = Router::new()
        .route("/api/v1/utxos", get(list_utxos))
        .route("/api/v1/commitments", get(list_commitments))
        .route("/api/v1/nullifiers", get(list_nullifiers))
        .route("/api/v1/blocks", get(list_blocks))
        .with_state(domain);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Starting Midnight API server on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn list_utxos(
    State(domain): State<MidnightDomain>,
) -> Result<Json<Vec<UtxoResponse>>, StatusCode> {
    let iter = domain
        .state
        .iter_entities_typed::<UnshieldedUtxo>(UTXO_NAMESPACE, Some(EntityKey::full_range()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = Vec::new();
    for res in iter {
        if let Ok((_key, utxo)) = res {
            response.push(UtxoResponse {
                intent_hash: hex::encode(utxo.intent_hash),
                output_index: utxo.output_index,
                amount: utxo.amount.to_string(),
                owner: hex::encode(utxo.owner),
            });
        }
    }

    Ok(Json(response))
}

async fn list_commitments(
    State(domain): State<MidnightDomain>,
) -> Result<Json<Vec<CommitmentResponse>>, StatusCode> {
    let iter = domain
        .state
        .iter_entities_typed::<ZswapCommitment>(COMMITMENT_NAMESPACE, Some(EntityKey::full_range()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = Vec::new();
    for res in iter {
        if let Ok((_key, commitment)) = res {
            response.push(CommitmentResponse {
                hash: hex::encode(commitment.hash),
            });
        }
    }

    Ok(Json(response))
}

async fn list_nullifiers(
    State(domain): State<MidnightDomain>,
) -> Result<Json<Vec<NullifierResponse>>, StatusCode> {
    let iter = domain
        .state
        .iter_entities_typed::<ZswapNullifier>(NULLIFIER_NAMESPACE, Some(EntityKey::full_range()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = Vec::new();
    for res in iter {
        if let Ok((_key, nullifier)) = res {
            response.push(NullifierResponse {
                hash: hex::encode(nullifier.hash),
            });
        }
    }

    Ok(Json(response))
}

async fn list_blocks(
    State(domain): State<MidnightDomain>,
) -> Result<Json<Vec<BlockResponse>>, StatusCode> {
    // get_range might borrow domain.archive, so we iterate immediately
    let iter = domain
        .archive
        .get_range(None, None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = Vec::new();
    for (slot, body) in iter {
        response.push(BlockResponse {
            slot,
            size: body.len(),
        });
    }

    // Sort descending by slot so latest blocks are first
    response.sort_by(|a, b| b.slot.cmp(&a.slot));

    // Limit to latest 100 blocks
    response.truncate(100);

    Ok(Json(response))
}
