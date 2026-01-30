//! ZK layer for the Privacy-Preserving Health-Data Ledger.
//!
//! This crate contains:
//! - A SNARK circuit that proves shard-level aggregate statistics were computed from committed data.
//! - Prover + verifier orchestration.
//! - Serialization helpers for transporting proofs and public inputs.

pub mod constants;
pub mod circuit;
pub mod groth16;
pub mod types;
