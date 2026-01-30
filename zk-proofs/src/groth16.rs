//! Groth16 prover/verifier orchestration for the shard circuit.
//!
//! SECURITY NOTE (prototype): Groth16 requires a trusted setup that produces a proving key (PK)
//! and verifying key (VK). This prototype generates keys locally. In production, an MPC ceremony
//! (or a transparent system) should be used.

use crate::circuit::HealthShardCircuit;
use crate::constants::{poseidon_config, DEFAULT_SHARD_SIZE, NUM_BUCKETS};
use crate::types::{bucket_for_age, Record, ShardPublicInputs, ShardStats};
use ark_bn254::{Bn254, Fr};
use ark_crypto_primitives::sponge::poseidon::PoseidonSponge;
use ark_crypto_primitives::sponge::CryptographicSponge;
use ark_groth16::{Groth16, Proof, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::RngCore;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ZkError {
    #[error("invalid shard size: expected {expected}, got {got}")]
    InvalidShardSize { expected: usize, got: usize },

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("proof verification failed")]
    VerificationFailed,

    #[error("arkworks error: {0}")]
    Ark(String),
}

/// Compute (commitment, stats) for a shard.
///
/// This MUST match the circuit's logic.
pub fn compute_shard_commitment_and_stats<const N: usize>(records: &[Record]) -> Result<(Fr, ShardStats), ZkError> {
    if records.len() != N {
        return Err(ZkError::InvalidShardSize { expected: N, got: records.len() });
    }

    let cfg = poseidon_config();
    let mut sponge = PoseidonSponge::<Fr>::new(&cfg);

    let mut stats = ShardStats::zero();

    for r in records {
        sponge.absorb(&[Fr::from(r.age as u64), Fr::from(r.blood_glucose_mg_dl as u64)]);

        let b = bucket_for_age(r.age);
        stats.sum_glucose_by_bucket[b] += r.blood_glucose_mg_dl as u64;
        stats.count_by_bucket[b] += 1;
    }

    let commitment = sponge.squeeze_field_elements(1)[0];
    Ok((commitment, stats))
}

/// Convert (commitment, stats) to the public-input vector expected by Groth16.
///
/// ORDERING MUST MATCH the circuit's `new_input` allocation order.
pub fn shard_public_inputs_to_field_elems(commitment: Fr, stats: &ShardStats) -> Vec<Fr> {
    let mut v = Vec::with_capacity(1 + 2 * NUM_BUCKETS);
    v.push(commitment);
    for i in 0..NUM_BUCKETS {
        v.push(Fr::from(stats.sum_glucose_by_bucket[i]));
    }
    for i in 0..NUM_BUCKETS {
        v.push(Fr::from(stats.count_by_bucket[i]));
    }
    v
}

/// Generate a Groth16 keypair for the shard circuit.
///
/// For a fixed `N`, this must be run once.
pub fn setup_keys<const N: usize>(rng: &mut impl RngCore) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>), ZkError> {
    // Use an empty witness; constraints only depend on N.
    let dummy_records = vec![Record { age: 0, blood_glucose_mg_dl: 0 }; N];
    let (commitment, stats) = compute_shard_commitment_and_stats::<N>(&dummy_records)?;

    let circuit = HealthShardCircuit::<N> {
        records: dummy_records,
        public_shard_commitment: commitment,
        public_sum_glucose_by_bucket: stats.sum_glucose_by_bucket,
        public_count_by_bucket: stats.count_by_bucket,
    };

    let pk = Groth16::<Bn254>::generate_random_parameters_with_reduction(circuit, rng)
        .map_err(|e| ZkError::Ark(format!("{e}")))?;

    let vk = pk.vk.clone();
    Ok((pk, vk))
}

/// Prove a shard's commitment and aggregate outputs.
pub fn prove_shard<const N: usize>(
    rng: &mut impl RngCore,
    pk: &ProvingKey<Bn254>,
    records: Vec<Record>,
) -> Result<(Proof<Bn254>, Fr, ShardStats), ZkError> {
    if records.len() != N {
        return Err(ZkError::InvalidShardSize { expected: N, got: records.len() });
    }

    let (commitment, stats) = compute_shard_commitment_and_stats::<N>(&records)?;

    let circuit = HealthShardCircuit::<N> {
        records,
        public_shard_commitment: commitment,
        public_sum_glucose_by_bucket: stats.sum_glucose_by_bucket,
        public_count_by_bucket: stats.count_by_bucket,
    };

    let proof = Groth16::<Bn254>::create_random_proof_with_reduction(circuit, pk, rng)
        .map_err(|e| ZkError::Ark(format!("{e}")))?;

    Ok((proof, commitment, stats))
}

/// Verify a shard proof.
pub fn verify_shard_proof(
    vk: &VerifyingKey<Bn254>,
    proof: &Proof<Bn254>,
    commitment: Fr,
    stats: &ShardStats,
) -> Result<(), ZkError> {
    let public_inputs = shard_public_inputs_to_field_elems(commitment, stats);
    let ok = Groth16::<Bn254>::verify_proof(vk, proof, &public_inputs)
        .map_err(|e| ZkError::Ark(format!("{e}")))?;
    if !ok {
        return Err(ZkError::VerificationFailed);
    }
    Ok(())
}

/// Serialize a proving key to bytes.
pub fn serialize_pk(pk: &ProvingKey<Bn254>) -> Result<Vec<u8>, ZkError> {
    let mut out = Vec::new();
    pk.serialize_compressed(&mut out)
        .map_err(|e| ZkError::Serialization(format!("{e}")))?;
    Ok(out)
}

pub fn deserialize_pk(bytes: &[u8]) -> Result<ProvingKey<Bn254>, ZkError> {
    ProvingKey::<Bn254>::deserialize_compressed(bytes)
        .map_err(|e| ZkError::Serialization(format!("{e}")))
}

pub fn serialize_vk(vk: &VerifyingKey<Bn254>) -> Result<Vec<u8>, ZkError> {
    let mut out = Vec::new();
    vk.serialize_compressed(&mut out)
        .map_err(|e| ZkError::Serialization(format!("{e}")))?;
    Ok(out)
}

pub fn deserialize_vk(bytes: &[u8]) -> Result<VerifyingKey<Bn254>, ZkError> {
    VerifyingKey::<Bn254>::deserialize_compressed(bytes)
        .map_err(|e| ZkError::Serialization(format!("{e}")))
}

pub fn serialize_proof(proof: &Proof<Bn254>) -> Result<Vec<u8>, ZkError> {
    let mut out = Vec::new();
    proof
        .serialize_compressed(&mut out)
        .map_err(|e| ZkError::Serialization(format!("{e}")))?;
    Ok(out)
}

pub fn deserialize_proof(bytes: &[u8]) -> Result<Proof<Bn254>, ZkError> {
    Proof::<Bn254>::deserialize_compressed(bytes)
        .map_err(|e| ZkError::Serialization(format!("{e}")))
}

/// Helper used by the backend for its default shard size.
pub type DefaultCircuit = HealthShardCircuit<DEFAULT_SHARD_SIZE>;

/// JSON-friendly public input bundle.
pub fn shard_public_inputs_json(commitment: Fr, stats: &ShardStats) -> ShardPublicInputs {
    ShardPublicInputs {
        shard_commitment: crate::types::FrHex::from_fr(&commitment),
        sum_glucose_by_bucket: stats.sum_glucose_by_bucket,
        count_by_bucket: stats.count_by_bucket,
    }
}
