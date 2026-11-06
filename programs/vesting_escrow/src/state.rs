//! The schedule account: one fixed-size record per vesting schedule, owned by this program and stored at a
//! program derived address (see `pda`). The layout is written by hand, little endian, with no padding between
//! fields and no serialization library; `docs/accounts.md` documents every offset.

use crate::codec::{Reader, Writer};
use crate::error::VestingError;
use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use vesting_core::Schedule;

/// First eight bytes of every schedule account: a tag with the layout version in the last byte. A zeroed
/// (closed) account never matches.
pub const DISCRIMINATOR: [u8; 8] = *b"vestsch1";

/// Size of the schedule account in bytes.
pub const SCHEDULE_ACCOUNT_SIZE: usize = 8 + 1 + 1 + 1 + 1 + 4 + 8 + 32 * 6 + 8 * 3 + 8 * 3 + 8;

/// A vesting schedule and everything the program needs to remember about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleAccount {
    /// Canonical bump of the schedule address, stored at creation and compared on every use.
    pub bump: u8,
    /// True if the authority may revoke.
    pub revocable: bool,
    /// True once a revocation has been executed.
    pub revoked: bool,
    /// Caller-chosen number that distinguishes the schedules of one creator and mint.
    pub id: u64,
    /// The account that signed the creation and funded the vault.
    pub creator: Pubkey,
    /// May revoke and close; receives the unvested remainder, any surplus in the vault and the rent.
    pub authority: Pubkey,
    /// May claim, and may hand the schedule to another key in two steps.
    pub beneficiary: Pubkey,
    /// A proposed new beneficiary, or the default key when no change is pending.
    pub pending_beneficiary: Pubkey,
    pub mint: Pubkey,
    /// The token account (the associated token account of the schedule address) that holds the tokens.
    pub vault: Pubkey,
    pub total_amount: u64,
    /// Paid out to the beneficiary so far.
    pub claimed_amount: u64,
    /// Returned to the authority by a revocation.
    pub returned_amount: u64,
    pub start_ts: i64,
    pub cliff_ts: i64,
    pub end_ts: i64,
    /// Time of the revocation, zero until then.
    pub revoked_at: i64,
}

impl ScheduleAccount {
    /// The pure schedule behind the stored numbers. Fails with `CorruptSchedule` if the stored parameters
    /// would not pass validation, which cannot happen to an account this program wrote.
    pub fn schedule(&self) -> Result<Schedule, VestingError> {
        Schedule::new(self.total_amount, self.start_ts, self.cliff_ts, self.end_ts).map_err(|_| VestingError::CorruptSchedule)
    }

    /// True when a beneficiary change has been proposed.
    pub fn has_pending_beneficiary(&self) -> bool {
        self.pending_beneficiary != Pubkey::default()
    }

    /// Parses the account data. The length must be exactly [`SCHEDULE_ACCOUNT_SIZE`] and the discriminator must
    /// match, so an account that is zeroed, truncated, padded or foreign is refused.
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != SCHEDULE_ACCOUNT_SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut r = Reader::new(data, ProgramError::InvalidAccountData);
        if r.bytes::<8>()? != DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        let bump = r.u8()?;
        let revocable = flag(r.u8()?)?;
        let revoked = flag(r.u8()?)?;
        if r.u8()? != 0 || r.bytes::<4>()? != [0; 4] {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(Self {
            bump,
            revocable,
            revoked,
            id: r.u64()?,
            creator: r.pubkey()?,
            authority: r.pubkey()?,
            beneficiary: r.pubkey()?,
            pending_beneficiary: r.pubkey()?,
            mint: r.pubkey()?,
            vault: r.pubkey()?,
            total_amount: r.u64()?,
            claimed_amount: r.u64()?,
            returned_amount: r.u64()?,
            start_ts: r.i64()?,
            cliff_ts: r.i64()?,
            end_ts: r.i64()?,
            revoked_at: r.i64()?,
        })
    }

    /// Writes the record into `dst`, which must be exactly [`SCHEDULE_ACCOUNT_SIZE`] bytes long.
    pub fn pack(&self, dst: &mut [u8]) -> Result<(), ProgramError> {
        if dst.len() != SCHEDULE_ACCOUNT_SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut w = Writer::new(dst, ProgramError::InvalidAccountData);
        w.put(&DISCRIMINATOR)?;
        w.u8(self.bump)?;
        w.u8(u8::from(self.revocable))?;
        w.u8(u8::from(self.revoked))?;
        w.u8(0)?;
        w.put(&[0; 4])?;
        w.u64(self.id)?;
        w.pubkey(&self.creator)?;
        w.pubkey(&self.authority)?;
        w.pubkey(&self.beneficiary)?;
        w.pubkey(&self.pending_beneficiary)?;
        w.pubkey(&self.mint)?;
        w.pubkey(&self.vault)?;
        w.u64(self.total_amount)?;
        w.u64(self.claimed_amount)?;
        w.u64(self.returned_amount)?;
        w.i64(self.start_ts)?;
        w.i64(self.cliff_ts)?;
        w.i64(self.end_ts)?;
        w.i64(self.revoked_at)
    }
}

/// A flag byte is 0 or 1; anything else means the data was not written by this program.
fn flag(byte: u8) -> Result<bool, ProgramError> {
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProgramError::InvalidAccountData),
    }
}
