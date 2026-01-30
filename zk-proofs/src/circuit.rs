//! R1CS circuit for proving shard-level aggregate correctness.
//!
//! What this circuit proves (for one shard):
//! 1) The prover knows N private records (age, glucose).
//! 2) A public commitment `C` equals Poseidon(records) (binding the proof to committed data).
//! 3) The public sums/counts for each age bucket equal the aggregates computed from those records.
//!
//! Privacy: the records are witnesses (never public). Only aggregates + commitment are public.

use crate::constants::{poseidon_config, AGE_BUCKETS, NUM_BUCKETS};
use crate::types::Record;
use ark_bn254::Fr;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_crypto_primitives::sponge::poseidon::PoseidonSponge;
use ark_crypto_primitives::sponge::{constraints::CryptographicSpongeVar, CryptographicSponge};
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

/// Convert little-endian boolean bits into an FpVar.
fn bits_le_to_fp(bits_le: &[Boolean<Fr>]) -> Result<FpVar<Fr>, SynthesisError> {
    let mut acc = FpVar::<Fr>::constant(Fr::from(0u64));
    let mut coeff = FpVar::<Fr>::constant(Fr::from(1u64));

    for b in bits_le {
        // b ? coeff : 0
        let term = b.select(&coeff, &FpVar::<Fr>::constant(Fr::from(0u64)))?;
        acc += term;
        coeff += coeff.clone();
    }

    Ok(acc)
}

/// Enforce that `v` is a u8 (fits in 8 bits) and return its 8 little-endian bits.
fn constrain_u8(v: &FpVar<Fr>) -> Result<Vec<Boolean<Fr>>, SynthesisError> {
    let bits = v.to_bits_le()?;
    let bits8 = bits[..8].to_vec();
    let reconstructed = bits_le_to_fp(&bits8)?;
    reconstructed.enforce_equal(v)?;
    Ok(bits8)
}

/// Enforce that `v` is a u16 (fits in 16 bits) and return its 16 little-endian bits.
fn constrain_u16(v: &FpVar<Fr>) -> Result<Vec<Boolean<Fr>>, SynthesisError> {
    let bits = v.to_bits_le()?;
    let bits16 = bits[..16].to_vec();
    let reconstructed = bits_le_to_fp(&bits16)?;
    reconstructed.enforce_equal(v)?;
    Ok(bits16)
}

/// Boolean gadget: `a <= c` where `a` is an 8-bit unsigned value in little-endian bits.
fn leq_const_u8(a_bits_le: &[Boolean<Fr>], c: u8) -> Result<Boolean<Fr>, SynthesisError> {
    // Lexicographic compare from MSB to LSB.
    let mut less = Boolean::constant(false);
    let mut equal = Boolean::constant(true);

    for i in (0..8).rev() {
        let a_i = a_bits_le[i].clone();
        let c_i = ((c >> i) & 1u8) == 1u8;

        // equal && (!a_i) && c_i
        if c_i {
            let not_a = a_i.not();
            let less_i = equal.and(&not_a)?;
            less = less.or(&less_i)?;
        }

        // equal = equal && (a_i == c_i)
        let a_eq_ci = if c_i { a_i } else { a_i.not() };
        equal = equal.and(&a_eq_ci)?;
    }

    less.or(&equal)
}

/// Boolean gadget: `a >= c` where `a` is u8.
fn geq_const_u8(a_bits_le: &[Boolean<Fr>], c: u8) -> Result<Boolean<Fr>, SynthesisError> {
    if c == 0 {
        return Ok(Boolean::constant(true));
    }
    // a >= c  <=>  !(a <= c-1)
    let leq_prev = leq_const_u8(a_bits_le, c - 1)?;
    Ok(leq_prev.not())
}

/// Boolean gadget: `min <= a <= max` for u8 value.
fn in_range_u8(a_bits_le: &[Boolean<Fr>], min: u8, max: u8) -> Result<Boolean<Fr>, SynthesisError> {
    let ge = geq_const_u8(a_bits_le, min)?;
    let le = leq_const_u8(a_bits_le, max)?;
    ge.and(&le)
}

/// Circuit proving shard commitment binding and bucketed aggregates.
///
/// `N` is the number of records in the shard.
#[derive(Clone, Debug)]
pub struct HealthShardCircuit<const N: usize> {
    /// Private records.
    pub records: Vec<Record>,

    /// Public commitment to the shard's records.
    pub public_shard_commitment: Fr,

    /// Public aggregate outputs.
    pub public_sum_glucose_by_bucket: [u64; NUM_BUCKETS],
    pub public_count_by_bucket: [u64; NUM_BUCKETS],
}

impl<const N: usize> ConstraintSynthesizer<Fr> for HealthShardCircuit<N> {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // --- Public inputs ---
        // These are what the verifier checks.
        let public_commitment = FpVar::<Fr>::new_input(cs.clone(), || Ok(self.public_shard_commitment))?;

        // IMPORTANT: Public input ordering MUST match `groth16::shard_public_inputs_to_field_elems`.
        // We use: commitment, sums[0..B), counts[0..B).
        let mut public_sums = Vec::<FpVar<Fr>>::with_capacity(NUM_BUCKETS);
        let mut public_counts = Vec::<FpVar<Fr>>::with_capacity(NUM_BUCKETS);

        for i in 0..NUM_BUCKETS {
            public_sums.push(FpVar::<Fr>::new_input(cs.clone(), || Ok(Fr::from(self.public_sum_glucose_by_bucket[i])))?);
        }
        for i in 0..NUM_BUCKETS {
            public_counts.push(FpVar::<Fr>::new_input(cs.clone(), || Ok(Fr::from(self.public_count_by_bucket[i])))?);
        }

        // --- Witness (private) records ---
        if self.records.len() != N {
            return Err(SynthesisError::Unsatisfiable);
        }

        let poseidon_cfg = poseidon_config();
        let mut sponge = PoseidonSpongeVar::<Fr>::new(cs.clone(), &poseidon_cfg);

        // Running aggregates.
        let mut sum_vars = vec![FpVar::<Fr>::constant(Fr::from(0u64)); NUM_BUCKETS];
        let mut count_vars = vec![FpVar::<Fr>::constant(Fr::from(0u64)); NUM_BUCKETS];

        for rec in self.records {
            // Allocate age and glucose as field elements.
            let age = FpVar::<Fr>::new_witness(cs.clone(), || Ok(Fr::from(rec.age as u64)))?;
            let glucose = FpVar::<Fr>::new_witness(cs.clone(), || Ok(Fr::from(rec.blood_glucose_mg_dl as u64)))?;

            // Range constrain to avoid ambiguous representations.
            let age_bits = constrain_u8(&age)?;
            let _glucose_bits = constrain_u16(&glucose)?;

            // Commitment binding: absorb private fields.
            sponge.absorb(&[age.clone(), glucose.clone()])?;

            // Bucket membership and aggregates.
            //
            // IMPORTANT: Every bucket constraint is explicit and non-overlapping.
            // The record contributes to exactly one bucket.
            let mut in_any_bucket = Boolean::constant(false);
            for (b, (min_age, max_age)) in AGE_BUCKETS.iter().enumerate() {
                let in_bucket = in_range_u8(&age_bits, *min_age, *max_age)?;
                in_any_bucket = in_any_bucket.or(&in_bucket)?;

                // sum_b += in_bucket ? glucose : 0
                let add_glucose = in_bucket.select(&glucose, &FpVar::<Fr>::constant(Fr::from(0u64)))?;
                sum_vars[b] += add_glucose;

                // count_b += in_bucket ? 1 : 0
                let add_one = in_bucket.select(&FpVar::<Fr>::constant(Fr::from(1u64)), &FpVar::<Fr>::constant(Fr::from(0u64)))?;
                count_vars[b] += add_one;
            }

            // Enforce that every age falls into some configured bucket.
            // (Buckets cover [0, 120], and the synthetic generator only emits ages in that range.)
            in_any_bucket.enforce_equal(&Boolean::constant(true))?;
        }

        // Squeeze the Poseidon sponge to derive the shard commitment.
        // This binds the aggregates to the committed records.
        let commitment = sponge.squeeze_field_elements(1)?[0].clone();
        commitment.enforce_equal(&public_commitment)?;

        // Enforce public outputs match computed aggregates.
        for i in 0..NUM_BUCKETS {
            sum_vars[i].enforce_equal(&public_sums[i])?;
            count_vars[i].enforce_equal(&public_counts[i])?;
        }

        // Optional: ensure the sponge isn't used elsewhere by accident.
        // (Not strictly needed, but helps prevent footguns when modifying circuit.)
        let _ = PoseidonSponge::<Fr>::new(&poseidon_cfg);

        Ok(())
    }
}
