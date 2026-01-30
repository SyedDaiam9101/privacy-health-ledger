use crate::{db, errors::ApiError};
use crate::state::AppState;
use base64::Engine;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use tracing::info;
use uuid::Uuid;
use zk_proofs::constants::DEFAULT_SHARD_SIZE;
use zk_proofs::groth16::{prove_shard, verify_shard_proof};
use zk_proofs::types::{Record, ShardStats};

use ark_bn254::Fr;
use ark_crypto_primitives::sponge::poseidon::PoseidonSponge;
use ark_crypto_primitives::sponge::CryptographicSponge;
use ark_serialize::CanonicalSerialize;
use zk_proofs::constants::poseidon_config;

/// Generate one synthetic record.
///
/// The generator is intentionally simple and deterministic.
fn gen_record(rng: &mut ChaCha20Rng) -> Record {
    let age = (rng.next_u32() % 121) as u8; // [0, 120]

    // Blood glucose: roughly [70, 180], uniform for the prototype.
    let glucose = 70u16 + (rng.next_u32() % 111) as u16;

    Record {
        age,
        blood_glucose_mg_dl: glucose,
    }
}

/// Derive a deterministic per-shard RNG seed.
///
/// This keeps dataset generation reproducible while allowing per-shard independent proving.
fn shard_seed(shard_index: u64) -> [u8; 32] {
    let mut seed = [0u8; 32];
    // Fixed domain separator for this prototype.
    seed[0..8].copy_from_slice(&0x485F4C4544474552u64.to_le_bytes()); // "H_LEDGER"ish
    seed[8..16].copy_from_slice(&shard_index.to_le_bytes());
    // Remaining bytes are constant.
    seed[16..].copy_from_slice(&[7u8; 16]);
    seed
}

/// Background job: generate the synthetic dataset, prove each shard, store in the ledger.
///
/// This NEVER writes raw records to disk and never exposes them via the API.
pub async fn generate_dataset_and_proofs(state: AppState, dataset_id: Uuid, dataset_size: u64) {
    let res = generate_dataset_and_proofs_inner(state.clone(), dataset_id, dataset_size).await;
    if let Err(e) = res {
        let _ = db::set_dataset_failed(&state.db, dataset_id, &format!("{e}"))
            .await;
    }
}

async fn generate_dataset_and_proofs_inner(
    state: AppState,
    dataset_id: Uuid,
    dataset_size: u64,
) -> Result<(), ApiError> {
    if dataset_size % (DEFAULT_SHARD_SIZE as u64) != 0 {
        return Err(ApiError::BadRequest(format!(
            "dataset_size must be a multiple of shard_size ({DEFAULT_SHARD_SIZE})"
        )));
    }

    let num_shards = dataset_size / (DEFAULT_SHARD_SIZE as u64);

    let keys = state.ensure_keys().await?;

    info!(%dataset_id, dataset_size, num_shards, "starting dataset generation");

    let poseidon_cfg = poseidon_config();
    let mut dataset_sponge = PoseidonSponge::<Fr>::new(&poseidon_cfg);

    for shard_index in 0..num_shards {
        let pk = keys.pk.clone();
        let vk = keys.vk.clone();

        // Generate + prove shard on a blocking thread.
        let (shard_commitment, stats, proof_b64, shard_commitment_hex) = tokio::task::spawn_blocking(move || {
            let mut record_rng = ChaCha20Rng::from_seed(shard_seed(shard_index));

            let mut records = Vec::with_capacity(DEFAULT_SHARD_SIZE);
            for _ in 0..DEFAULT_SHARD_SIZE {
                records.push(gen_record(&mut record_rng));
            }

            // Use OS randomness for the proof to avoid deterministic proofs.
            let mut proof_rng = rand::rngs::OsRng;
            let (proof, shard_commitment, stats) = prove_shard::<DEFAULT_SHARD_SIZE>(&mut proof_rng, pk.as_ref(), records)
                .map_err(|_| ApiError::Internal)?;

            // Fail closed if proof doesn't verify.
            verify_shard_proof(vk.as_ref(), &proof, shard_commitment, &stats).map_err(|_| ApiError::Internal)?;

            let b64 = base64::engine::general_purpose::STANDARD;
            let proof_bytes = zk_proofs::groth16::serialize_proof(&proof).map_err(|_| ApiError::Internal)?;
            let proof_b64 = b64.encode(proof_bytes);

            let mut commitment_bytes = Vec::new();
            shard_commitment
                .serialize_compressed(&mut commitment_bytes)
                .map_err(|_| ApiError::Internal)?;
            let shard_commitment_hex = hex::encode(commitment_bytes);

            Ok::<(Fr, ShardStats, String, String), ApiError>((shard_commitment, stats, proof_b64, shard_commitment_hex))
        })
        .await
        .map_err(|_| ApiError::Internal)??;

        // Update dataset commitment.
        dataset_sponge.absorb(&[shard_commitment]);

        // Persist shard.
        db::insert_shard(
            &state.db,
            dataset_id,
            shard_index,
            &shard_commitment_hex,
            &stats,
            &proof_b64,
            true,
        )
        .await?;

        if shard_index % 10 == 0 {
            info!(%dataset_id, shard_index, "generated shard");
        }
    }

    // Derive dataset commitment.
    let dataset_commitment = dataset_sponge.squeeze_field_elements(1)[0];
    let mut bytes = Vec::new();
    dataset_commitment
        .serialize_compressed(&mut bytes)
        .map_err(|_| ApiError::Internal)?;
    let dataset_commitment_hex = hex::encode(bytes);

    db::set_dataset_ready(&state.db, dataset_id, &dataset_commitment_hex).await?;

    info!(%dataset_id, "dataset ready");
    Ok(())
}
