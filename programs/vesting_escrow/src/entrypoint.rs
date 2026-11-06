//! The on-chain entrypoint. The macro generates the C ABI function that the loader calls; it is the only place
//! in the workspace that needs `unsafe`, and it is compiled out with the `no-entrypoint` feature (tests and
//! other crates that only use the types).
#![allow(unsafe_code)]

use solana_program::{account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey};

entrypoint!(process);

fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    crate::processor::process_instruction(program_id, accounts, data)
}
