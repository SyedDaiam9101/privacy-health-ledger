use crate::db;
use crate::errors::ApiError;
use crate::models::*;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;
use zk_proofs::constants::{AGE_BUCKETS, DEFAULT_SHARD_SIZE, NUM_BUCKETS};
use zk_proofs::groth16::{deserialize_proof, deserialize_vk, verify_shard_proof};
use zk_proofs::types::ShardStats;

use ark_bn254::Fr;
use ark_serialize::CanonicalDeserialize;

#[derive(Debug, serde::Deserialize)]
pub struct ListShardsParams {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
    pub include_proof: Option<bool>,
}

pub fn router(state: AppState) -> Router {
    let protected_routes = Router::new()
        .route("/api/v1/datasets", post(create_dataset))
        .route("/api/v1/queries", post(create_query))
        .route("/api/v1/verify/shard", post(verify_shard))
        .layer(middleware::from_fn(auth_middleware));

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/datasets/:id", get(get_dataset))
        .route("/api/v1/datasets/:id/shards", get(list_shards))
        .route("/api/v1/zk/vk", get(get_vk))
        .merge(protected_routes)
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}

async fn auth_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // In production, this should be a strong secret from environment.
    let expected_key = std::env::var("API_KEY").unwrap_or_else(|_| "dev-secret-key".to_string());

    if let Some(provided_key) = headers.get("X-API-KEY") {
        if provided_key == expected_key.as_str() {
            return Ok(next.run(request).await);
        }
    }

    tracing::warn!("unauthorized access attempt");
    Err(StatusCode::UNAUTHORIZED)
}

async fn create_dataset(State(state): State<AppState>, Json(req): Json<DatasetCreateRequest>) -> Result<Json<DatasetCreateResponse>, ApiError> {
    let dataset_size = req.dataset_size.unwrap_or(1_000_000);

    if dataset_size % (DEFAULT_SHARD_SIZE as u64) != 0 {
        return Err(ApiError::BadRequest(format!(
            "dataset_size must be a multiple of shard_size ({DEFAULT_SHARD_SIZE})"
        )));
    }

    let dataset_id = Uuid::new_v4();
    db::insert_dataset(&state.db, dataset_id, dataset_size).await?;

    // Start background generation.
    tokio::spawn(crate::dataset::generate_dataset_and_proofs(
        state.clone(),
        dataset_id,
        dataset_size,
    ));

    Ok(Json(DatasetCreateResponse { dataset_id }))
}

async fn get_dataset(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<DatasetGetResponse>, ApiError> {
    let Some((created_at, dataset_size, status_str, commitment, error)) = db::get_dataset(&state.db, id).await? else {
        return Err(ApiError::NotFound("dataset not found".to_string()));
    };

    let status = match status_str.as_str() {
        "generating" => DatasetStatus::Generating,
        "ready" => DatasetStatus::Ready,
        "failed" => DatasetStatus::Failed,
        _ => DatasetStatus::Failed,
    };

    let shards_total = dataset_size / (DEFAULT_SHARD_SIZE as u64);
    let shards_done = db::count_shards_done(&state.db, id).await?;

    Ok(Json(DatasetGetResponse {
        dataset_id: id,
        created_at,
        dataset_size,
        shard_size: DEFAULT_SHARD_SIZE as u64,
        num_buckets: NUM_BUCKETS as u64,
        status,
        shards_total,
        shards_done,
        dataset_commitment_hex: commitment,
        error,
    }))
}

async fn list_shards(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<ListShardsParams>,
) -> Result<Json<ShardListResponse>, ApiError> {
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50).min(500);
    let include_proof = params.include_proof.unwrap_or(false);

    let Some((_created_at, dataset_size, _status, _commitment, _error)) = db::get_dataset(&state.db, id).await? else {
        return Err(ApiError::NotFound("dataset not found".to_string()));
    };
    let shards_total = dataset_size / (DEFAULT_SHARD_SIZE as u64);

    let rows = db::list_shards(&state.db, id, offset, limit, include_proof).await?;

    let mut shards = Vec::with_capacity(rows.len());
    for (shard_index, commitment_hex, stats, verified, proof_b64) in rows {
        shards.push(ShardListItem {
            shard_index,
            shard_commitment_hex: commitment_hex,
            sum_glucose_by_bucket: stats.sum_glucose_by_bucket,
            count_by_bucket: stats.count_by_bucket,
            verified,
            proof_b64,
        });
    }

    Ok(Json(ShardListResponse {
        dataset_id: id,
        offset,
        limit,
        shards_total,
        shards,
    }))
}

async fn create_query(State(state): State<AppState>, Json(req): Json<QueryRequest>) -> Result<Json<QueryResponse>, ApiError> {
    if req.field != "blood_glucose" && req.field != "blood_glucose_mg_dl" {
        return Err(ApiError::BadRequest("only field 'blood_glucose' is supported".to_string()));
    }

    let bucket_index = bucket_for_age_range(&req.age_range)
        .ok_or_else(|| ApiError::BadRequest("age_range must match one of the configured buckets".to_string()))?;

    // Ensure dataset exists.
    let Some((_created_at, dataset_size, status, _commitment, _error)) = db::get_dataset(&state.db, req.dataset_id).await? else {
        return Err(ApiError::NotFound("dataset not found".to_string()));
    };

    if status != "ready" {
        return Err(ApiError::Conflict("dataset not ready".to_string()));
    }

    let (sum, count) = db::aggregate_for_bucket(&state.db, req.dataset_id, bucket_index).await?;

    let mean = match req.metric {
        Metric::Mean => {
            if count == 0 {
                None
            } else {
                Some(sum as f64 / count as f64)
            }
        }
        _ => None,
    };

    // Server-side verification: all shards must be verified.
    let shards_total = dataset_size / (DEFAULT_SHARD_SIZE as u64);
    let shards_verified = db::count_shards_verified(&state.db, req.dataset_id).await?;
    let server_verified = shards_verified == shards_total;

    let query_id = Uuid::new_v4();
    db::insert_query(
        &state.db,
        query_id,
        req.dataset_id,
        &req.metric,
        bucket_index,
        sum,
        count,
        mean,
        server_verified,
    )
    .await?;

    let (min_age, max_age) = AGE_BUCKETS[bucket_index];

    Ok(Json(QueryResponse {
        query_id,
        dataset_id: req.dataset_id,
        bucket_index,
        bucket_range: (min_age, max_age),
        sum_glucose: sum,
        count,
        mean_glucose: match req.metric {
            Metric::Mean => mean,
            Metric::Sum => None,
            Metric::Count => None,
        },
        server_verified,
        shard_proofs_endpoint: format!("/api/v1/datasets/{}/shards?include_proof=true", req.dataset_id),
    }))
}

async fn get_vk(State(state): State<AppState>) -> Result<Json<ZkVkResponse>, ApiError> {
    let keys = state.ensure_keys().await?;
    let vk_bytes = zk_proofs::groth16::serialize_vk(keys.vk.as_ref()).map_err(|_| ApiError::Internal)?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(vk_bytes);

    Ok(Json(ZkVkResponse {
        curve: "bn254".to_string(),
        proof_system: "groth16".to_string(),
        vk_b64: b64,
    }))
}

async fn verify_shard(State(_state): State<AppState>, Json(req): Json<VerifyShardRequest>) -> Result<Json<VerifyShardResponse>, ApiError> {
    let b64 = base64::engine::general_purpose::STANDARD;

    let vk_bytes = b64.decode(req.vk_b64).map_err(|_| ApiError::BadRequest("invalid vk_b64".to_string()))?;
    let proof_bytes = b64.decode(req.proof_b64).map_err(|_| ApiError::BadRequest("invalid proof_b64".to_string()))?;

    let vk = deserialize_vk(&vk_bytes).map_err(|_| ApiError::BadRequest("invalid vk".to_string()))?;
    let proof = deserialize_proof(&proof_bytes).map_err(|_| ApiError::BadRequest("invalid proof".to_string()))?;

    // Commitment is stored as hex-encoded compressed field element bytes.
    let commitment_bytes = hex::decode(req.public_shard_commitment_hex)
        .map_err(|_| ApiError::BadRequest("invalid commitment hex".to_string()))?;
    let commitment = Fr::deserialize_compressed(&commitment_bytes[..])
        .map_err(|_| ApiError::BadRequest("invalid commitment bytes".to_string()))?;

    let stats = ShardStats {
        sum_glucose_by_bucket: req.public_sum_glucose_by_bucket,
        count_by_bucket: req.public_count_by_bucket,
    };

    let ok = verify_shard_proof(&vk, &proof, commitment, &stats).is_ok();

    Ok(Json(VerifyShardResponse { ok }))
}
