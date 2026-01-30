use crate::errors::ApiError;
use crate::models::Metric;
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use uuid::Uuid;
use zk_proofs::constants::{DEFAULT_SHARD_SIZE, NUM_BUCKETS};
use zk_proofs::types::ShardStats;

pub type Db = Pool<Sqlite>;

pub async fn connect(db_url: &str) -> Result<Db, ApiError> {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await
        .map_err(|_| ApiError::Internal)
}

pub async fn init_schema(db: &Db) -> Result<(), ApiError> {
    // NOTE: Keep schema minimal and explicit. This is an append-only-ish ledger prototype.
    sqlx::query(
        r#"
CREATE TABLE IF NOT EXISTS datasets (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  dataset_size INTEGER NOT NULL,
  shard_size INTEGER NOT NULL,
  num_buckets INTEGER NOT NULL,
  status TEXT NOT NULL,
  dataset_commitment_hex TEXT,
  error TEXT
);

CREATE TABLE IF NOT EXISTS shards (
  dataset_id TEXT NOT NULL,
  shard_index INTEGER NOT NULL,
  shard_commitment_hex TEXT NOT NULL,
  stats_json TEXT NOT NULL,
  proof_b64 TEXT NOT NULL,
  verified INTEGER NOT NULL,
  PRIMARY KEY(dataset_id, shard_index)
);

CREATE TABLE IF NOT EXISTS queries (
  id TEXT PRIMARY KEY,
  dataset_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  query_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  verified INTEGER NOT NULL
);
"#,
    )
    .execute(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(())
}

pub async fn insert_dataset(db: &Db, dataset_id: Uuid, dataset_size: u64) -> Result<(), ApiError> {
    let created_at = Utc::now().to_rfc3339();
    let status = "generating";

    sqlx::query(
        r#"INSERT INTO datasets (id, created_at, dataset_size, shard_size, num_buckets, status)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(dataset_id.to_string())
    .bind(created_at)
    .bind(dataset_size as i64)
    .bind(DEFAULT_SHARD_SIZE as i64)
    .bind(NUM_BUCKETS as i64)
    .bind(status)
    .execute(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(())
}

pub async fn set_dataset_ready(db: &Db, dataset_id: Uuid, commitment_hex: &str) -> Result<(), ApiError> {
    sqlx::query(r#"UPDATE datasets SET status = 'ready', dataset_commitment_hex = ?, error = NULL WHERE id = ?"#)
        .bind(commitment_hex)
        .bind(dataset_id.to_string())
        .execute(db)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(())
}

pub async fn set_dataset_failed(db: &Db, dataset_id: Uuid, error: &str) -> Result<(), ApiError> {
    sqlx::query(r#"UPDATE datasets SET status = 'failed', error = ? WHERE id = ?"#)
        .bind(error)
        .bind(dataset_id.to_string())
        .execute(db)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(())
}

pub async fn insert_shard(
    db: &Db,
    dataset_id: Uuid,
    shard_index: u64,
    shard_commitment_hex: &str,
    stats: &ShardStats,
    proof_b64: &str,
    verified: bool,
) -> Result<(), ApiError> {
    let stats_json = serde_json::to_string(stats).map_err(|_| ApiError::Internal)?;

    sqlx::query(
        r#"INSERT OR REPLACE INTO shards
           (dataset_id, shard_index, shard_commitment_hex, stats_json, proof_b64, verified)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(dataset_id.to_string())
    .bind(shard_index as i64)
    .bind(shard_commitment_hex)
    .bind(stats_json)
    .bind(proof_b64)
    .bind(if verified { 1i64 } else { 0i64 })
    .execute(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(())
}

pub async fn get_dataset(db: &Db, dataset_id: Uuid) -> Result<Option<(DateTime<Utc>, u64, String, Option<String>, Option<String>)>, ApiError> {
    let row = sqlx::query(
        r#"SELECT created_at, dataset_size, status, dataset_commitment_hex, error
           FROM datasets WHERE id = ?"#,
    )
    .bind(dataset_id.to_string())
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let Some(row) = row else { return Ok(None); };

    let created_at: String = row.get(0);
    let created_at = DateTime::parse_from_rfc3339(&created_at)
        .map_err(|_| ApiError::Internal)?
        .with_timezone(&Utc);

    let dataset_size: i64 = row.get(1);
    let status: String = row.get(2);
    let commitment_hex: Option<String> = row.get(3);
    let error: Option<String> = row.get(4);

    Ok(Some((created_at, dataset_size as u64, status, commitment_hex, error)))
}

pub async fn count_shards_done(db: &Db, dataset_id: Uuid) -> Result<u64, ApiError> {
    let row = sqlx::query(r#"SELECT COUNT(*) AS c FROM shards WHERE dataset_id = ?"#)
        .bind(dataset_id.to_string())
        .fetch_one(db)
        .await
        .map_err(|_| ApiError::Internal)?;
    let c: i64 = row.get("c");
    Ok(c as u64)
}

pub async fn count_shards_verified(db: &Db, dataset_id: Uuid) -> Result<u64, ApiError> {
    let row = sqlx::query(r#"SELECT COUNT(*) AS c FROM shards WHERE dataset_id = ? AND verified = 1"#)
        .bind(dataset_id.to_string())
        .fetch_one(db)
        .await
        .map_err(|_| ApiError::Internal)?;
    let c: i64 = row.get("c");
    Ok(c as u64)
}

pub async fn list_shards(
    db: &Db,
    dataset_id: Uuid,
    offset: u64,
    limit: u64,
    include_proof: bool,
) -> Result<Vec<(u64, String, ShardStats, bool, Option<String>)>, ApiError> {
    let rows = sqlx::query(
        r#"SELECT shard_index, shard_commitment_hex, stats_json, verified, proof_b64
           FROM shards
           WHERE dataset_id = ?
           ORDER BY shard_index
           LIMIT ? OFFSET ?"#,
    )
    .bind(dataset_id.to_string())
    .bind(limit as i64)
    .bind(offset as i64)
    .fetch_all(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let shard_index: i64 = row.get(0);
        let commitment: String = row.get(1);
        let stats_json: String = row.get(2);
        let verified: i64 = row.get(3);
        let proof_b64: String = row.get(4);

        let stats: ShardStats = serde_json::from_str(&stats_json).map_err(|_| ApiError::Internal)?;

        out.push((
            shard_index as u64,
            commitment,
            stats,
            verified == 1,
            if include_proof { Some(proof_b64) } else { None },
        ));
    }

    Ok(out)
}

pub async fn aggregate_for_bucket(
    db: &Db,
    dataset_id: Uuid,
    bucket_index: usize,
) -> Result<(u64, u64), ApiError> {
    if bucket_index >= NUM_BUCKETS {
        return Err(ApiError::BadRequest("invalid bucket".to_string()));
    }

    let rows = sqlx::query(r#"SELECT stats_json FROM shards WHERE dataset_id = ?"#)
        .bind(dataset_id.to_string())
        .fetch_all(db)
        .await
        .map_err(|_| ApiError::Internal)?;

    let mut sum = 0u64;
    let mut count = 0u64;

    for row in rows {
        let stats_json: String = row.get(0);
        let stats: ShardStats = serde_json::from_str(&stats_json).map_err(|_| ApiError::Internal)?;
        sum += stats.sum_glucose_by_bucket[bucket_index];
        count += stats.count_by_bucket[bucket_index];
    }

    Ok((sum, count))
}

pub async fn insert_query(
    db: &Db,
    query_id: Uuid,
    dataset_id: Uuid,
    metric: &Metric,
    bucket_index: usize,
    sum: u64,
    count: u64,
    mean: Option<f64>,
    verified: bool,
) -> Result<(), ApiError> {
    let created_at = Utc::now().to_rfc3339();

    let query_json = json!({
        "metric": metric,
        "bucket_index": bucket_index,
        "field": "blood_glucose_mg_dl"
    });
    let result_json = json!({
        "sum_glucose": sum,
        "count": count,
        "mean_glucose": mean
    });

    sqlx::query(
        r#"INSERT INTO queries (id, dataset_id, created_at, query_json, result_json, verified)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(query_id.to_string())
    .bind(dataset_id.to_string())
    .bind(created_at)
    .bind(query_json.to_string())
    .bind(result_json.to_string())
    .bind(if verified { 1i64 } else { 0i64 })
    .execute(db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(())
}
