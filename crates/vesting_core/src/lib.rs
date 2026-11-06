//! Pure vesting arithmetic for the escrow program.
//!
//! This crate knows nothing about Solana: no account types, no public keys, no clock account. It takes plain
//! integers, validates a vesting schedule and answers three questions with checked arithmetic:
//!
//! * how much has vested at a given Unix time ([`Schedule::vested_at`]),
//! * how much can be claimed given what was claimed before ([`Schedule::claimable`]),
//! * how a revocation splits the escrowed amount ([`Schedule::revocation`]).
//!
//! Rounding always favours the vault: the vested amount is the floor of the exact linear amount, so a
//! beneficiary can never be paid more than the exact schedule allows, and the full amount is released only
//! at the end time.
#![forbid(unsafe_code)]

mod error;
mod schedule;

pub use error::{AccountingError, ScheduleError};
pub use schedule::{Schedule, MAX_DURATION_SECS};
