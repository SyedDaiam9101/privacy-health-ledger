//! Crate-wide constants used by the ZK circuit and host-side orchestration.

use ark_bn254::Fr;
use ark_crypto_primitives::sponge::poseidon::{find_poseidon_ark_and_mds, PoseidonConfig};
use ark_ff::PrimeField;

/// Default number of records per shard.
///
/// We choose 1000 so the canonical "1,000,000 record" synthetic dataset partitions into exactly
/// 1000 shards.
pub const DEFAULT_SHARD_SIZE: usize = 1000;

/// Number of age buckets used by the prototype.
pub const NUM_BUCKETS: usize = 6;

/// Inclusive (min_age, max_age) bounds for each bucket.
///
/// Buckets cover [0, 120] and are designed for the demo query:
/// "Average blood glucose by age range".
pub const AGE_BUCKETS: [(u8, u8); NUM_BUCKETS] = [
    (0, 17),
    (18, 29),
    (30, 39),
    (40, 49),
    (50, 64),
    (65, 120),
];

// Poseidon sponge configuration.
//
// We use a width-3 sponge (rate=2, capacity=1) to efficiently absorb pairs of field elements.
// The specific round counts chosen here are consistent with widely used Poseidon instantiations.
//
// NOTE: This is a prototype. For production, parameters should be reviewed by cryptographers
// and ideally fixed via audited constants / standard sets.
pub const POSEIDON_RATE: usize = 2;
pub const POSEIDON_CAPACITY: usize = 1;

// Typical Poseidon parameters for width=3.
pub const POSEIDON_FULL_ROUNDS: usize = 8;
pub const POSEIDON_PARTIAL_ROUNDS: usize = 57;

/// Poseidon S-box exponent (alpha). Common choices are 5 or 17.
pub const POSEIDON_ALPHA: u64 = 5;

/// Deterministically derive Poseidon parameters for BN254::Fr.
///
/// This uses arkworks' parameter derivation helper (Ark + MDS) so both the native hasher
/// and the in-circuit gadget agree on the same constants.
pub fn poseidon_config() -> PoseidonConfig<Fr> {
    // The helper expects the prime field size in bits.
    let prime_bits = Fr::MODULUS_BIT_SIZE as u64;

    // Derive the round constants (ARK) and MDS matrix.
    let (ark, mds) = find_poseidon_ark_and_mds::<Fr>(
        prime_bits,
        POSEIDON_RATE,
        POSEIDON_FULL_ROUNDS,
        POSEIDON_PARTIAL_ROUNDS,
        0,
    );

    PoseidonConfig::new(
        POSEIDON_FULL_ROUNDS,
        POSEIDON_PARTIAL_ROUNDS,
        POSEIDON_ALPHA,
        mds,
        ark,
        POSEIDON_RATE,
        POSEIDON_CAPACITY,
    )
}
