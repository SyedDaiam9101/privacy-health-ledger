use crate::errors::ApiError;
use crate::db::Db;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use zk_proofs::constants::DEFAULT_SHARD_SIZE;
use zk_proofs::groth16::{deserialize_pk, deserialize_vk, serialize_pk, serialize_vk, setup_keys};

use ark_bn254::Bn254;
use ark_groth16::{ProvingKey, VerifyingKey};
use rand::rngs::OsRng;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub data_dir: PathBuf,
    keys: Arc<OnceCell<ZkKeys>>,
}

#[derive(Clone)]
pub struct ZkKeys {
    pub pk: Arc<ProvingKey<Bn254>>,
    pub vk: Arc<VerifyingKey<Bn254>>,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf) -> Self {
        Self {
            db,
            data_dir,
            keys: Arc::new(OnceCell::new()),
        }
    }

    /// Ensure Groth16 keys exist on disk and in memory.
    ///
    /// This runs the trusted setup (prototype) on first use.
    pub async fn ensure_keys(&self) -> Result<ZkKeys, ApiError> {
        let data_dir = self.data_dir.clone();

        self.keys
            .get_or_try_init(|| async move {
                tokio::task::spawn_blocking(move || {
                    let keys_dir = data_dir.join("keys");
                    std::fs::create_dir_all(&keys_dir).map_err(|_| ApiError::Internal)?;

                    let pk_path = keys_dir.join("groth16_pk.bin");
                    let vk_path = keys_dir.join("groth16_vk.bin");

                    if pk_path.exists() && vk_path.exists() {
                        let pk_bytes = std::fs::read(&pk_path).map_err(|_| ApiError::Internal)?;
                        let vk_bytes = std::fs::read(&vk_path).map_err(|_| ApiError::Internal)?;

                        let pk = deserialize_pk(&pk_bytes).map_err(|_| ApiError::Internal)?;
                        let vk = deserialize_vk(&vk_bytes).map_err(|_| ApiError::Internal)?;

                        return Ok::<ZkKeys, ApiError>(ZkKeys { pk: Arc::new(pk), vk: Arc::new(vk) });
                    }

                    // Trusted setup randomness (prototype).
                    //
                    // IMPORTANT: In production, use MPC setup or a transparent proof system.
                    let mut rng = OsRng;
                    let (pk, vk) = setup_keys::<DEFAULT_SHARD_SIZE>(&mut rng).map_err(|_| ApiError::Internal)?;

                    let pk_bytes = serialize_pk(&pk).map_err(|_| ApiError::Internal)?;
                    let vk_bytes = serialize_vk(&vk).map_err(|_| ApiError::Internal)?;

                    std::fs::write(&pk_path, pk_bytes).map_err(|_| ApiError::Internal)?;
                    std::fs::write(&vk_path, vk_bytes).map_err(|_| ApiError::Internal)?;

                    Ok::<ZkKeys, ApiError>(ZkKeys { pk: Arc::new(pk), vk: Arc::new(vk) })
                })
                .await
                .map_err(|_| ApiError::Internal)?
            })
            .await
            .cloned()
    }
}
