use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zk_proofs::constants::{AGE_BUCKETS, NUM_BUCKETS};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetStatus {
    Generating,
    Ready,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DatasetCreateRequest {
    /// Total number of synthetic records to commit.
    ///
    /// Must be a multiple of the shard size (1000 in the default build).
    pub dataset_size: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DatasetCreateResponse {
    pub dataset_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DatasetGetResponse {
    pub dataset_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub dataset_size: u64,
    pub shard_size: u64,
    pub num_buckets: u64,
    pub status: DatasetStatus,
    pub shards_total: u64,
    pub shards_done: u64,
    pub dataset_commitment_hex: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Count,
    Sum,
    Mean,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgeRange {
    pub min_age: u8,
    pub max_age: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryRequest {
    pub dataset_id: Uuid,
    pub metric: Metric,

    /// The prototype supports a single field: blood glucose.
    pub field: String,

    /// Filter: age range must match one of the configured buckets.
    pub age_range: AgeRange,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResponse {
    pub query_id: Uuid,
    pub dataset_id: Uuid,

    pub bucket_index: usize,
    pub bucket_range: (u8, u8),

    pub sum_glucose: u64,
    pub count: u64,
    pub mean_glucose: Option<f64>,

    /// Indicates whether all shard proofs backing this dataset have been verified by the backend.
    pub server_verified: bool,

    /// Where a researcher can fetch shard proofs and public inputs for independent verification.
    pub shard_proofs_endpoint: String,
}

pub fn bucket_for_age_range(range: &AgeRange) -> Option<usize> {
    for (i, (min, max)) in AGE_BUCKETS.iter().enumerate() {
        if range.min_age == *min && range.max_age == *max {
            return Some(i);
        }
    }
    None
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShardListResponse {
    pub dataset_id: Uuid,
    pub offset: u64,
    pub limit: u64,
    pub shards_total: u64,
    pub shards: Vec<ShardListItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShardListItem {
    pub shard_index: u64,
    pub shard_commitment_hex: String,

    pub sum_glucose_by_bucket: [u64; NUM_BUCKETS],
    pub count_by_bucket: [u64; NUM_BUCKETS],

    pub verified: bool,

    /// Included only if requested (large).
    pub proof_b64: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ZkVkResponse {
    pub curve: String,
    pub proof_system: String,
    pub vk_b64: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyShardRequest {
    pub vk_b64: String,
    pub proof_b64: String,

    pub public_shard_commitment_hex: String,
    pub public_sum_glucose_by_bucket: [u64; NUM_BUCKETS],
    pub public_count_by_bucket: [u64; NUM_BUCKETS],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyShardResponse {
    pub ok: bool,
}
