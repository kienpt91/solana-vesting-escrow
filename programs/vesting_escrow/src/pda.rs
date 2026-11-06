//! Address derivation. The schedule address is a program derived address, so only this program can sign for it,
//! and the vault is the associated token account of that address, so its location is fixed by the schedule.

use solana_program::pubkey::Pubkey;

/// First seed of every schedule address.
pub const SCHEDULE_SEED: &[u8] = b"vesting";

/// The associated token account program.
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = solana_program::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// The canonical schedule address and its bump for a creator, a mint and a schedule id under `program_id`.
/// Seeds: `"vesting" || creator || mint || id (u64, little endian)`. The creator is part of the seeds so
/// that nobody else can occupy an address the creator is about to use.
pub fn schedule_address(program_id: &Pubkey, creator: &Pubkey, mint: &Pubkey, id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SCHEDULE_SEED, creator.as_ref(), mint.as_ref(), &id.to_le_bytes()], program_id)
}

/// The vault of a schedule: the associated token account of the schedule address for the mint, under the
/// SPL Token program.
pub fn vault_address(schedule: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[schedule.as_ref(), spl_token::ID.as_ref(), mint.as_ref()], &ASSOCIATED_TOKEN_PROGRAM_ID).0
}
