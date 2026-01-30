//! Types shared between the circuit and the host-side prover/verifier.

use crate::constants::{AGE_BUCKETS, NUM_BUCKETS};
use ark_bn254::Fr;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use serde::{Deserialize, Serialize};

/// One synthetic health record.
///
/// IMPORTANT: This is *synthetic* and intentionally minimal for the prototype.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    /// Age in years.
    pub age: u8,
    /// Blood glucose (mg/dL).
    pub blood_glucose_mg_dl: u16,
}

/// A shard's aggregate statistics, bucketed by age.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShardStats {
    /// Sum of blood glucose per age bucket.
    pub sum_glucose_by_bucket: [u64; NUM_BUCKETS],
    /// Count of records per age bucket.
    pub count_by_bucket: [u64; NUM_BUCKETS],
}

impl ShardStats {
    pub fn zero() -> Self {
        Self {
            sum_glucose_by_bucket: [0u64; NUM_BUCKETS],
            count_by_bucket: [0u64; NUM_BUCKETS],
        }
    }
}

/// JSON-friendly representation of a field element.
///
/// We expose Fr values as hex strings (big-endian) to avoid ambiguities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrHex {
    pub hex: String,
}

impl FrHex {
    pub fn from_fr(x: &Fr) -> Self {
        // Use arkworks' canonical compressed encoding so all components agree.
        let mut bytes = Vec::new();
        x.serialize_compressed(&mut bytes)
            .expect("in-memory serialization");
        Self { hex: hex::encode(bytes) }
    }

    pub fn to_fr(&self) -> Result<Fr, String> {
        let bytes = hex::decode(&self.hex).map_err(|e| format!("invalid hex: {e}"))?;
        Fr::deserialize_compressed(&bytes[..]).map_err(|e| format!("invalid field bytes: {e}"))
    }
}

/// Public inputs for a shard proof.
///
/// Ordering MUST match the circuit's public input allocation order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShardPublicInputs {
    pub shard_commitment: FrHex,
    pub sum_glucose_by_bucket: [u64; NUM_BUCKETS],
    pub count_by_bucket: [u64; NUM_BUCKETS],
}

/// Convenience: map an age to a bucket index.
///
/// Used by the host to compute expected public outputs (sum/count) that the circuit will enforce.
pub fn bucket_for_age(age: u8) -> usize {
    for (i, (min, max)) in AGE_BUCKETS.iter().enumerate() {
        if age >= *min && age <= *max {
            return i;
        }
    }
    // Ages outside the configured range are clamped to the last bucket.
    NUM_BUCKETS - 1
}
