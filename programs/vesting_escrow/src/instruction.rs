//! Instruction data: one tag byte followed by fixed-size little-endian fields. The length is checked exactly, so
//! trailing bytes are refused. In particular no instruction carries a time: the program reads the clock sysvar.

use crate::codec::{Reader, Writer};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

/// Parameters of `InitializeSchedule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializeSchedule {
    pub schedule_id: u64,
    pub beneficiary: Pubkey,
    pub authority: Pubkey,
    pub total_amount: u64,
    pub start_ts: i64,
    pub cliff_ts: i64,
    pub end_ts: i64,
    pub revocable: bool,
}

/// An instruction of the program. See `docs/accounts.md` for the account list of each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VestingInstruction {
    /// Creates the schedule account and moves `total_amount` tokens from the creator into the vault.
    InitializeSchedule(InitializeSchedule),
    /// Pays the beneficiary what has vested and was not claimed yet.
    Claim,
    /// The authority stops a revocable schedule: the vested part goes to the beneficiary, the rest comes back.
    Revoke,
    /// The authority removes a settled schedule and its vault and takes the rent back.
    Close,
    /// The current beneficiary proposes another key.
    ProposeBeneficiary { new_beneficiary: Pubkey },
    /// The proposed key accepts and becomes the beneficiary.
    AcceptBeneficiary,
}

const TAG_INITIALIZE: u8 = 0;
const TAG_CLAIM: u8 = 1;
const TAG_REVOKE: u8 = 2;
const TAG_CLOSE: u8 = 3;
const TAG_PROPOSE: u8 = 4;
const TAG_ACCEPT: u8 = 5;

/// Exact data length of `InitializeSchedule`: tag, id, two keys, total, three times, flag.
pub const INITIALIZE_DATA_LEN: usize = 1 + 8 + 32 + 32 + 8 + 8 + 8 + 8 + 1;
/// Exact data length of `ProposeBeneficiary`.
pub const PROPOSE_DATA_LEN: usize = 1 + 32;

impl VestingInstruction {
    /// Parses instruction data. Unknown tags, short data, trailing bytes and a flag other than 0 or 1 are
    /// `InvalidInstructionData`.
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        let err = ProgramError::InvalidInstructionData;
        let mut r = Reader::new(data, err.clone());
        let instruction = match r.u8()? {
            TAG_INITIALIZE => {
                let schedule_id = r.u64()?;
                let beneficiary = r.pubkey()?;
                let authority = r.pubkey()?;
                let total_amount = r.u64()?;
                let start_ts = r.i64()?;
                let cliff_ts = r.i64()?;
                let end_ts = r.i64()?;
                let revocable = match r.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(err),
                };
                Self::InitializeSchedule(InitializeSchedule {
                    schedule_id,
                    beneficiary,
                    authority,
                    total_amount,
                    start_ts,
                    cliff_ts,
                    end_ts,
                    revocable,
                })
            }
            TAG_CLAIM => Self::Claim,
            TAG_REVOKE => Self::Revoke,
            TAG_CLOSE => Self::Close,
            TAG_PROPOSE => Self::ProposeBeneficiary { new_beneficiary: r.pubkey()? },
            TAG_ACCEPT => Self::AcceptBeneficiary,
            _ => return Err(err),
        };
        if r.remaining() != 0 {
            return Err(err);
        }
        Ok(instruction)
    }

    /// Serializes the instruction; the inverse of [`VestingInstruction::unpack`].
    pub fn pack(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Self::InitializeSchedule(p) => {
                let mut buf = [0u8; INITIALIZE_DATA_LEN];
                // the buffer has exactly the size of the fields, so the writer cannot run out of room
                let mut w = Writer::new(&mut buf, ProgramError::InvalidInstructionData);
                let written = w
                    .u8(TAG_INITIALIZE)
                    .and_then(|()| w.u64(p.schedule_id))
                    .and_then(|()| w.pubkey(&p.beneficiary))
                    .and_then(|()| w.pubkey(&p.authority))
                    .and_then(|()| w.u64(p.total_amount))
                    .and_then(|()| w.i64(p.start_ts))
                    .and_then(|()| w.i64(p.cliff_ts))
                    .and_then(|()| w.i64(p.end_ts))
                    .and_then(|()| w.u8(u8::from(p.revocable)));
                debug_assert!(written.is_ok());
                out.extend_from_slice(&buf);
            }
            Self::Claim => out.push(TAG_CLAIM),
            Self::Revoke => out.push(TAG_REVOKE),
            Self::Close => out.push(TAG_CLOSE),
            Self::ProposeBeneficiary { new_beneficiary } => {
                out.push(TAG_PROPOSE);
                out.extend_from_slice(new_beneficiary.as_ref());
            }
            Self::AcceptBeneficiary => out.push(TAG_ACCEPT),
        }
        out
    }
}
