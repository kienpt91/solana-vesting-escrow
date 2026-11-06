//! Client SDK of the vesting escrow program: address derivation, instruction builders and account decoding.
//!
//! Nothing here talks to a cluster. The builders return plain `Instruction`s; sending them is the caller's
//! business. Every builder derives the vault address from the schedule address and the mint, so a caller can
//! not pass a vault that does not belong to the schedule by mistake (the program refuses one anyway).

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program,
};

pub use vesting_core::{AccountingError, Revocation, Schedule, ScheduleError, MAX_DURATION_SECS};
pub use vesting_escrow::error::VestingError;
pub use vesting_escrow::instruction::{InitializeSchedule, VestingInstruction};
pub use vesting_escrow::pda::{schedule_address, vault_address, ASSOCIATED_TOKEN_PROGRAM_ID};
pub use vesting_escrow::state::{ScheduleAccount, SCHEDULE_ACCOUNT_SIZE};
pub use vesting_escrow::ID;

/// Everything `initialize_schedule` needs to know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializeParams {
    /// Signs, pays the rent and funds the vault from `source_token`.
    pub creator: Pubkey,
    /// Distinguishes the schedules of one creator and mint.
    pub schedule_id: u64,
    pub mint: Pubkey,
    /// A token account of the creator that holds at least `total_amount` of the mint.
    pub source_token: Pubkey,
    pub beneficiary: Pubkey,
    pub authority: Pubkey,
    pub total_amount: u64,
    pub start_ts: i64,
    pub cliff_ts: i64,
    pub end_ts: i64,
    pub revocable: bool,
}

impl InitializeParams {
    /// The address of the schedule these parameters create.
    pub fn schedule(&self, program_id: &Pubkey) -> Pubkey {
        schedule_address(program_id, &self.creator, &self.mint, self.schedule_id).0
    }
}

/// Creates the vault (the associated token account of the schedule address) if it does not exist yet. Send it
/// in the same transaction, before `initialize_schedule`. It is idempotent: it succeeds if the vault exists.
pub fn create_vault(payer: &Pubkey, schedule: &Pubkey, mint: &Pubkey) -> Instruction {
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(vault_address(schedule, mint), false),
            AccountMeta::new_readonly(*schedule, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        // CreateIdempotent
        data: vec![1],
    }
}

/// `initialize_schedule`: creates the schedule account and moves `total_amount` into the vault.
pub fn initialize_schedule(program_id: &Pubkey, p: &InitializeParams) -> Instruction {
    let schedule = p.schedule(program_id);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(p.creator, true),
            AccountMeta::new(schedule, false),
            AccountMeta::new_readonly(p.mint, false),
            AccountMeta::new(p.source_token, false),
            AccountMeta::new(vault_address(&schedule, &p.mint), false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: VestingInstruction::InitializeSchedule(InitializeSchedule {
            schedule_id: p.schedule_id,
            beneficiary: p.beneficiary,
            authority: p.authority,
            total_amount: p.total_amount,
            start_ts: p.start_ts,
            cliff_ts: p.cliff_ts,
            end_ts: p.end_ts,
            revocable: p.revocable,
        })
        .pack(),
    }
}

/// `claim`: the beneficiary receives everything vested and not yet claimed in `beneficiary_token`.
pub fn claim(program_id: &Pubkey, beneficiary: &Pubkey, schedule: &Pubkey, mint: &Pubkey, beneficiary_token: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new_readonly(*beneficiary, true),
            AccountMeta::new(*schedule, false),
            AccountMeta::new(vault_address(schedule, mint), false),
            AccountMeta::new(*beneficiary_token, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: VestingInstruction::Claim.pack(),
    }
}

/// `revoke`: the vested part goes to `beneficiary_token`, the unvested remainder to `authority_token`.
pub fn revoke(
    program_id: &Pubkey,
    authority: &Pubkey,
    schedule: &Pubkey,
    mint: &Pubkey,
    beneficiary_token: &Pubkey,
    authority_token: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*schedule, false),
            AccountMeta::new(vault_address(schedule, mint), false),
            AccountMeta::new(*beneficiary_token, false),
            AccountMeta::new(*authority_token, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: VestingInstruction::Revoke.pack(),
    }
}

/// `close`: removes a settled schedule and its vault; surplus tokens go to `authority_token`, the rent to the
/// authority.
pub fn close(program_id: &Pubkey, authority: &Pubkey, schedule: &Pubkey, mint: &Pubkey, authority_token: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(*schedule, false),
            AccountMeta::new(vault_address(schedule, mint), false),
            AccountMeta::new(*authority_token, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: VestingInstruction::Close.pack(),
    }
}

/// `propose_beneficiary`: step one of a beneficiary change, signed by the current beneficiary.
pub fn propose_beneficiary(program_id: &Pubkey, beneficiary: &Pubkey, schedule: &Pubkey, new_beneficiary: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![AccountMeta::new_readonly(*beneficiary, true), AccountMeta::new(*schedule, false)],
        data: VestingInstruction::ProposeBeneficiary { new_beneficiary: *new_beneficiary }.pack(),
    }
}

/// `accept_beneficiary`: step two, signed by the proposed key.
pub fn accept_beneficiary(program_id: &Pubkey, new_beneficiary: &Pubkey, schedule: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![AccountMeta::new_readonly(*new_beneficiary, true), AccountMeta::new(*schedule, false)],
        data: VestingInstruction::AcceptBeneficiary.pack(),
    }
}

/// Decodes the data of a schedule account.
pub fn decode_schedule(data: &[u8]) -> Result<ScheduleAccount, ProgramError> {
    ScheduleAccount::unpack(data)
}

/// What a schedule looks like at a point in time, computed with the same pure logic the program uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub vested: u64,
    pub claimed: u64,
    /// What `claim` would pay now; zero for a revoked schedule.
    pub claimable: u64,
    /// Tokens that have not vested yet (zero after a revocation).
    pub unvested: u64,
}

/// The status of a schedule at `now` (Unix seconds).
pub fn status(state: &ScheduleAccount, now: i64) -> Result<Status, ProgramError> {
    let core = state.schedule()?;
    if state.revoked {
        return Ok(Status { vested: state.claimed_amount, claimed: state.claimed_amount, claimable: 0, unvested: 0 });
    }
    let vested = core.vested_at(now).map_err(vesting_escrow::error::lift)?;
    let claimable = core.claimable(now, state.claimed_amount).map_err(vesting_escrow::error::lift)?;
    let unvested = state.total_amount.checked_sub(vested).ok_or(ProgramError::from(VestingError::MathOverflow))?;
    Ok(Status { vested, claimed: state.claimed_amount, claimable, unvested })
}

/// Names the custom error code of a failed instruction, for logs and tables.
pub fn describe_error(code: u32) -> String {
    match VestingError::from_code(code) {
        Some(e) => format!("{e:?} ({code}): {e}"),
        None => format!("code {code} (not an error of this program)"),
    }
}
