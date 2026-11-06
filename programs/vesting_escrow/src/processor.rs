//! The six instructions. Every handler follows the same order: account list shape, signers and writability,
//! program ids, schedule state (owner, layout, canonical address), role of each signer, token accounts, then the
//! time-dependent rule, then the state change, then the token CPI. The state is written before the CPI; the CPI
//! targets (the SPL Token program and the system program) never call back into this program, so there is no
//! re-entrancy to guard against.
//!
//! Money arithmetic is not done here: amounts come out of `vesting_core`.

use crate::error::{lift, VestingError};
use crate::instruction::{InitializeSchedule, VestingInstruction};
use crate::pda::{self, SCHEDULE_SEED};
use crate::state::{ScheduleAccount, SCHEDULE_ACCOUNT_SIZE};
use crate::token;
use crate::validation::{self, distinct, key_is, owned_by, signer, writable};
use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};
use vesting_core::Schedule;

/// Entry point shared by the on-chain entrypoint and by in-process tests.
pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match VestingInstruction::unpack(data)? {
        VestingInstruction::InitializeSchedule(params) => {
            msg!("vesting_escrow: initialize_schedule");
            initialize_schedule(program_id, accounts, params)
        }
        VestingInstruction::Claim => {
            msg!("vesting_escrow: claim");
            claim(program_id, accounts)
        }
        VestingInstruction::Revoke => {
            msg!("vesting_escrow: revoke");
            revoke(program_id, accounts)
        }
        VestingInstruction::Close => {
            msg!("vesting_escrow: close");
            close(program_id, accounts)
        }
        VestingInstruction::ProposeBeneficiary { new_beneficiary } => {
            msg!("vesting_escrow: propose_beneficiary");
            propose_beneficiary(program_id, accounts, new_beneficiary)
        }
        VestingInstruction::AcceptBeneficiary => {
            msg!("vesting_escrow: accept_beneficiary");
            accept_beneficiary(program_id, accounts)
        }
    }
}

/// The time of the clock sysvar. It is the only source of time; no instruction carries one.
fn now() -> Result<i64, ProgramError> {
    let unix_timestamp = Clock::get()?.unix_timestamp;
    if unix_timestamp < 0 {
        return Err(VestingError::InvalidClock.into());
    }
    Ok(unix_timestamp)
}

/// The signer seeds of a schedule address: the fixed prefix, creator, mint, id and the stored bump.
struct SignerSeeds {
    id: [u8; 8],
    bump: [u8; 1],
    creator: Pubkey,
    mint: Pubkey,
}

impl SignerSeeds {
    fn new(state: &ScheduleAccount) -> Self {
        Self { id: state.id.to_le_bytes(), bump: [state.bump], creator: state.creator, mint: state.mint }
    }

    fn slices(&self) -> [&[u8]; 5] {
        [SCHEDULE_SEED, self.creator.as_ref(), self.mint.as_ref(), &self.id, &self.bump]
    }
}

/// Reads and checks the schedule account: owned by this program, exact layout, and located at the canonical
/// address for its creator, mint and id with the bump it stores.
fn load_schedule(program_id: &Pubkey, schedule: &AccountInfo) -> Result<ScheduleAccount, ProgramError> {
    owned_by(schedule, program_id, VestingError::WrongAccountOwner)?;
    let state = ScheduleAccount::unpack(&schedule.try_borrow_data()?)?;
    let (address, bump) = pda::schedule_address(program_id, &state.creator, &state.mint, state.id);
    if address != *schedule.key || bump != state.bump {
        return Err(VestingError::WrongScheduleAddress.into());
    }
    Ok(state)
}

fn store_schedule(schedule: &AccountInfo, state: &ScheduleAccount) -> ProgramResult {
    state.pack(&mut schedule.try_borrow_mut_data()?)
}

/// The mint and the vault of an existing schedule, checked against what the schedule recorded.
struct Escrow {
    decimals: u8,
    vault_amount: u64,
}

fn load_escrow(state: &ScheduleAccount, schedule: &AccountInfo, vault: &AccountInfo, mint: &AccountInfo) -> Result<Escrow, ProgramError> {
    key_is(mint, &state.mint, VestingError::WrongMint)?;
    let mint_state = token::load_mint(mint)?;
    key_is(vault, &state.vault, VestingError::WrongVault)?;
    let vault_state = token::load_account(vault)?;
    token::check_vault(&vault_state, &state.mint, schedule.key)?;
    Ok(Escrow { decimals: mint_state.decimals, vault_amount: vault_state.amount })
}

// ---------------------------------------------------------------------------------------------------------
// initialize_schedule
// ---------------------------------------------------------------------------------------------------------

fn initialize_schedule(program_id: &Pubkey, accounts: &[AccountInfo], p: InitializeSchedule) -> ProgramResult {
    let [creator, schedule, mint, source, vault, system_program_account, token_program] = validation::exact::<7>(accounts)?;
    signer(creator)?;
    writable(creator)?;
    writable(schedule)?;
    writable(source)?;
    writable(vault)?;
    key_is(system_program_account, &system_program::ID, VestingError::WrongSystemProgram)?;
    token::check_program(token_program)?;
    distinct(&[creator, schedule, mint, source, vault])?;

    if p.beneficiary == Pubkey::default() || p.authority == Pubkey::default() {
        return Err(VestingError::InvalidParty.into());
    }
    let now = now()?;
    let core = Schedule::new_at(p.total_amount, p.start_ts, p.cliff_ts, p.end_ts, now).map_err(lift)?;

    let mint_state = token::load_mint(mint)?;

    let (address, bump) = pda::schedule_address(program_id, creator.key, mint.key, p.schedule_id);
    key_is(schedule, &address, VestingError::WrongScheduleAddress)?;
    // a schedule address that already holds data, or belongs to another program, is never overwritten
    if *schedule.owner != system_program::ID || !schedule.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    key_is(vault, &pda::vault_address(schedule.key, mint.key), VestingError::WrongVault)?;
    let vault_before = token::load_account(vault)?;
    token::check_vault(&vault_before, mint.key, schedule.key)?;

    let source_state = token::load_account(source)?;
    token::check_account(&source_state, mint.key, creator.key)?;
    if source_state.amount < core.total() {
        return Err(VestingError::InsufficientFunds.into());
    }

    let state = ScheduleAccount {
        bump,
        revocable: p.revocable,
        revoked: false,
        id: p.schedule_id,
        creator: *creator.key,
        authority: p.authority,
        beneficiary: p.beneficiary,
        pending_beneficiary: Pubkey::default(),
        mint: *mint.key,
        vault: *vault.key,
        total_amount: core.total(),
        claimed_amount: 0,
        returned_amount: 0,
        start_ts: core.start(),
        cliff_ts: core.cliff(),
        end_ts: core.end(),
        revoked_at: 0,
    };

    let seeds = SignerSeeds::new(&state);
    create_schedule_account(program_id, creator, schedule, system_program_account, &seeds.slices())?;
    store_schedule(schedule, &state)?;
    if !Rent::get()?.is_exempt(schedule.lamports(), SCHEDULE_ACCOUNT_SIZE) {
        return Err(ProgramError::AccountNotRentExempt);
    }

    token::transfer(token_program, source, mint, vault, creator, core.total(), mint_state.decimals)?;

    // the vault must have grown by exactly the total: no fee, no hook, no surprise
    let expected = vault_before.amount.checked_add(core.total()).ok_or(VestingError::MathOverflow)?;
    if token::load_account(vault)?.amount != expected {
        return Err(VestingError::VaultFundingMismatch.into());
    }
    Ok(())
}

/// Creates the schedule account at its derived address. If somebody sent lamports to the address first, the
/// address cannot be created with `CreateAccount`; it is topped up to rent exemption, allocated and assigned
/// instead, so that a donation cannot block a schedule.
fn create_schedule_account<'info>(
    program_id: &Pubkey,
    payer: &AccountInfo<'info>,
    schedule: &AccountInfo<'info>,
    system_program_account: &AccountInfo<'info>,
    seeds: &[&[u8]],
) -> ProgramResult {
    let required = Rent::get()?.minimum_balance(SCHEDULE_ACCOUNT_SIZE);
    let size = SCHEDULE_ACCOUNT_SIZE as u64;
    if schedule.lamports() == 0 {
        return invoke_signed(
            &system_instruction::create_account(payer.key, schedule.key, required, size, program_id),
            &[payer.clone(), schedule.clone(), system_program_account.clone()],
            &[seeds],
        );
    }
    let missing = required.saturating_sub(schedule.lamports());
    if missing > 0 {
        invoke(
            &system_instruction::transfer(payer.key, schedule.key, missing),
            &[payer.clone(), schedule.clone(), system_program_account.clone()],
        )?;
    }
    invoke_signed(&system_instruction::allocate(schedule.key, size), &[schedule.clone(), system_program_account.clone()], &[seeds])?;
    invoke_signed(&system_instruction::assign(schedule.key, program_id), &[schedule.clone(), system_program_account.clone()], &[seeds])
}

// ---------------------------------------------------------------------------------------------------------
// claim
// ---------------------------------------------------------------------------------------------------------

fn claim(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [beneficiary, schedule, vault, beneficiary_token, mint, token_program] = validation::exact::<6>(accounts)?;
    signer(beneficiary)?;
    writable(schedule)?;
    writable(vault)?;
    writable(beneficiary_token)?;
    token::check_program(token_program)?;
    distinct(&[beneficiary, schedule, vault, beneficiary_token, mint])?;

    let mut state = load_schedule(program_id, schedule)?;
    key_is(beneficiary, &state.beneficiary, VestingError::NotBeneficiary)?;
    let escrow = load_escrow(&state, schedule, vault, mint)?;
    let destination = token::load_account(beneficiary_token)?;
    token::check_account(&destination, &state.mint, &state.beneficiary)?;

    if state.revoked {
        return Err(VestingError::ScheduleRevoked.into());
    }
    let core = state.schedule()?;
    let now = now()?;
    if now < core.cliff() {
        return Err(VestingError::CliffNotReached.into());
    }
    let amount = core.claimable(now, state.claimed_amount).map_err(lift)?;
    if amount == 0 {
        return Err(VestingError::NothingToClaim.into());
    }
    state.claimed_amount = core.after_claim(state.claimed_amount, amount).map_err(lift)?;
    store_schedule(schedule, &state)?;

    let seeds = SignerSeeds::new(&state);
    token::transfer_from_vault(token_program, vault, mint, beneficiary_token, schedule, amount, escrow.decimals, &seeds.slices())
}

// ---------------------------------------------------------------------------------------------------------
// revoke
// ---------------------------------------------------------------------------------------------------------

fn revoke(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [authority, schedule, vault, beneficiary_token, authority_token, mint, token_program] = validation::exact::<7>(accounts)?;
    signer(authority)?;
    writable(schedule)?;
    writable(vault)?;
    writable(beneficiary_token)?;
    writable(authority_token)?;
    token::check_program(token_program)?;
    // both payees may be the same account (the authority and the beneficiary can be one wallet); neither may
    // be the vault, the schedule, the mint or the signer
    distinct(&[authority, schedule, vault, mint, beneficiary_token])?;
    distinct(&[authority, schedule, vault, mint, authority_token])?;

    let mut state = load_schedule(program_id, schedule)?;
    key_is(authority, &state.authority, VestingError::NotAuthority)?;
    if !state.revocable {
        return Err(VestingError::NotRevocable.into());
    }
    if state.revoked {
        return Err(VestingError::ScheduleRevoked.into());
    }
    let escrow = load_escrow(&state, schedule, vault, mint)?;
    let to_authority_account = token::load_account(authority_token)?;
    token::check_account(&to_authority_account, &state.mint, &state.authority)?;

    let core = state.schedule()?;
    let now = now()?;
    if core.is_fully_vested(now) {
        return Err(VestingError::FullyVested.into());
    }
    let split = core.revocation(now, state.claimed_amount).map_err(lift)?;
    // the beneficiary's account matters only if it is paid: a revocation before the cliff owes the beneficiary
    // nothing, and an account that cannot receive tokens (frozen, say) must not stall the authority's recovery
    if split.to_beneficiary > 0 {
        let to_beneficiary_account = token::load_account(beneficiary_token)?;
        token::check_account(&to_beneficiary_account, &state.mint, &state.beneficiary)?;
    }

    state.revoked = true;
    state.revoked_at = now;
    state.claimed_amount = core.after_claim(state.claimed_amount, split.to_beneficiary).map_err(lift)?;
    state.returned_amount = split.to_authority;
    // nothing is owed to a beneficiary after this: a proposal made before the revocation is dropped, not left open
    state.pending_beneficiary = Pubkey::default();
    store_schedule(schedule, &state)?;

    let seeds = SignerSeeds::new(&state);
    if split.to_beneficiary > 0 {
        token::transfer_from_vault(
            token_program,
            vault,
            mint,
            beneficiary_token,
            schedule,
            split.to_beneficiary,
            escrow.decimals,
            &seeds.slices(),
        )?;
    }
    if split.to_authority > 0 {
        token::transfer_from_vault(
            token_program,
            vault,
            mint,
            authority_token,
            schedule,
            split.to_authority,
            escrow.decimals,
            &seeds.slices(),
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------
// close
// ---------------------------------------------------------------------------------------------------------

fn close(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [authority, schedule, vault, authority_token, mint, token_program] = validation::exact::<6>(accounts)?;
    signer(authority)?;
    writable(authority)?;
    writable(schedule)?;
    writable(vault)?;
    writable(authority_token)?;
    token::check_program(token_program)?;
    distinct(&[authority, schedule, vault, mint, authority_token])?;

    let state = load_schedule(program_id, schedule)?;
    key_is(authority, &state.authority, VestingError::NotAuthority)?;
    let escrow = load_escrow(&state, schedule, vault, mint)?;
    let destination = token::load_account(authority_token)?;
    token::check_account(&destination, &state.mint, &state.authority)?;

    // everything owed has left the vault: claimed by the beneficiary or returned to the authority
    let core = state.schedule()?;
    if !core.is_settled(state.claimed_amount, state.returned_amount).map_err(lift)? {
        return Err(VestingError::NotSettled.into());
    }

    let seeds = SignerSeeds::new(&state);
    // anything still in the vault was sent there by someone else (the schedule never needs it): it goes to the
    // authority, so a donation cannot keep the vault from being closed
    if escrow.vault_amount > 0 {
        token::transfer_from_vault(
            token_program,
            vault,
            mint,
            authority_token,
            schedule,
            escrow.vault_amount,
            escrow.decimals,
            &seeds.slices(),
        )?;
    }
    token::close_vault(token_program, vault, authority, schedule, &seeds.slices())?;
    close_program_account(schedule, authority)
}

/// Closes an account owned by this program: the data is zeroed, every lamport goes to `destination`, the owner
/// becomes the system program and the data length becomes zero. The account then holds nothing for the runtime
/// to keep, and a later transaction in which the address is reused finds a plain empty system account.
fn close_program_account(account: &AccountInfo, destination: &AccountInfo) -> ProgramResult {
    account.try_borrow_mut_data()?.fill(0);
    {
        let mut from = account.try_borrow_mut_lamports()?;
        let mut to = destination.try_borrow_mut_lamports()?;
        **to = to.checked_add(**from).ok_or(VestingError::MathOverflow)?;
        **from = 0;
    }
    account.assign(&system_program::ID);
    account.realloc(0, false)
}

// ---------------------------------------------------------------------------------------------------------
// beneficiary change (two steps)
// ---------------------------------------------------------------------------------------------------------

fn propose_beneficiary(program_id: &Pubkey, accounts: &[AccountInfo], new_beneficiary: Pubkey) -> ProgramResult {
    let [beneficiary, schedule] = validation::exact::<2>(accounts)?;
    signer(beneficiary)?;
    writable(schedule)?;

    let mut state = load_schedule(program_id, schedule)?;
    key_is(beneficiary, &state.beneficiary, VestingError::NotBeneficiary)?;
    if new_beneficiary == Pubkey::default() || new_beneficiary == state.beneficiary {
        return Err(VestingError::InvalidParty.into());
    }
    state.pending_beneficiary = new_beneficiary;
    store_schedule(schedule, &state)
}

fn accept_beneficiary(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [new_beneficiary, schedule] = validation::exact::<2>(accounts)?;
    signer(new_beneficiary)?;
    writable(schedule)?;

    let mut state = load_schedule(program_id, schedule)?;
    if !state.has_pending_beneficiary() {
        return Err(VestingError::NoPendingBeneficiary.into());
    }
    key_is(new_beneficiary, &state.pending_beneficiary, VestingError::NotPendingBeneficiary)?;
    state.beneficiary = state.pending_beneficiary;
    state.pending_beneficiary = Pubkey::default();
    store_schedule(schedule, &state)
}
