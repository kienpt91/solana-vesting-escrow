//! Vesting escrow program: locks SPL tokens in a vault controlled by a program derived address and releases them
//! on a schedule (cliff, then linear), with optional revocation by an authority.
//!
//! * `vesting_core` does the money arithmetic (this crate does none of its own).
//! * `state` is the schedule account layout, `instruction` the instruction data, `pda` the address derivation.
//! * `validation` and `token` are the account-checking layer; `processor` holds the six instructions.
//!
//! Only the classic SPL Token program is supported. See `docs/` for the account layouts, the threat model and
//! the audit checklist. This program has not been audited and has not been deployed to any cluster.
#![deny(unsafe_code)]

mod codec;
pub mod error;
pub mod instruction;
pub mod pda;
pub mod state;

use solana_program::pubkey::Pubkey;

/// A placeholder program id (ASCII text, no keypair exists for it). Deploying means choosing a real id; every
/// derivation in the program uses the id the runtime passes in, never this constant.
pub const ID: Pubkey = Pubkey::new_from_array(*b"VestingEscrowDemoProgram11111111");
