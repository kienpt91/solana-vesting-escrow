//! The attack suite: hostile transactions against a funded schedule. Every test names the attack, expects one precise
//! error at one precise instruction, and checks that the failed transaction changed nothing. Each test prints a
//! line `ATTACK|name|expected|observed|verdict` that `scripts/demo.sh` collects into a table.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use solana_program::{program_pack::Pack, pubkey::Pubkey};
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction, InstructionError},
    signature::Keypair,
    signer::Signer,
    system_instruction, system_program,
    sysvar::clock::Clock,
};
use spl_token::error::TokenError;
use spl_token::state::{Account as TokenAccountState, AccountState};
use vesting_client::{self as client, VestingError, ID};
use vesting_escrow::pda::SCHEDULE_SEED;
use vesting_tests::attack::{assert_rejected, assert_rejected_by, assert_rejected_with};
use vesting_tests::{token_account_to_account, Failure, Scenario, Setup, DAY, T0, TOKEN_2022_ID};

// byte offsets of the schedule account layout (docs/accounts.md)
const OFFSET_BUMP: usize = 8;
const OFFSET_CLAIMED: usize = 224;

fn clones(keys: &[&Keypair]) -> Vec<Keypair> {
    keys.iter().map(|k| k.insecure_clone()).collect()
}

/// Sends one hostile instruction signed by `signers`, expects exactly `error` at `index`, and checks that every
/// watched account is unchanged afterwards.
async fn reject(s: &mut Scenario, name: &str, ix: Instruction, signers: &[&Keypair], index: u8, error: VestingError) {
    let watched = s.watched();
    let before = s.env.snapshot(&watched).await;
    let signers = clones(signers);
    let refs: Vec<&Keypair> = signers.iter().collect();
    let result = s.env.send(&[ix], &refs).await;
    assert_rejected(name, result, index, error);
    s.env.assert_unchanged(&watched, &before).await;
}

/// Like [`reject`] for a built-in program error.
async fn reject_with(s: &mut Scenario, name: &str, ix: Instruction, signers: &[&Keypair], index: u8, error: InstructionError) {
    let watched = s.watched();
    let before = s.env.snapshot(&watched).await;
    let signers = clones(signers);
    let refs: Vec<&Keypair> = signers.iter().collect();
    let result = s.env.send(&[ix], &refs).await;
    assert_rejected_with(name, result, index, error);
    s.env.assert_unchanged(&watched, &before).await;
}

/// Like [`reject_with`] for a hostile instruction addressed straight to another program (the token program, the system
/// program): the error and the program that must return it.
async fn reject_by(
    s: &mut Scenario,
    name: &str,
    ix: Instruction,
    signers: &[&Keypair],
    index: u8,
    program: &Pubkey,
    error: InstructionError,
) {
    let watched = s.watched();
    let before = s.env.snapshot(&watched).await;
    let signers = clones(signers);
    let refs: Vec<&Keypair> = signers.iter().collect();
    let result = s.env.send(&[ix], &refs).await;
    assert_rejected_by(name, result, index, program, error);
    s.env.assert_unchanged(&watched, &before).await;
}

/// The custom error code of a token program error.
fn token_error(e: TokenError) -> InstructionError {
    InstructionError::Custom(e as u32)
}

/// A funded schedule, halfway through its vesting.
async fn halfway() -> Scenario {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(500).await;
    s
}

/// The creation attempts send the vault creation first (so the vault exists) and the hostile initialize second.
async fn init_attack(s: &mut Scenario, hostile: Instruction) -> Result<(), Failure> {
    let creator = s.creator.insecure_clone();
    let ixs = [client::create_vault(&creator.pubkey(), &s.schedule, &s.mint), hostile];
    s.env.send(&ixs, &[&creator]).await
}

/// Rewrites the schedule account's data in place.
async fn patch_schedule(s: &mut Scenario, f: impl FnOnce(&mut Account)) {
    let mut account = s.env.account(&s.schedule).await.unwrap();
    f(&mut account);
    s.env.set_account(&s.schedule, account);
}

/// A valid program-derived address for the same seeds with a lower, non-canonical bump.
fn non_canonical(creator: &Pubkey, mint: &Pubkey, id: u64) -> Option<(Pubkey, u8)> {
    let (_, canonical) = client::schedule_address(&ID, creator, mint, id);
    (0..canonical).rev().find_map(|bump| {
        Pubkey::create_program_address(&[SCHEDULE_SEED, creator.as_ref(), mint.as_ref(), &id.to_le_bytes(), &[bump]], &ID)
            .ok()
            .map(|a| (a, bump))
    })
}

// ---------------------------------------------------------------------------------------------------------
// signers
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn wrong_signer_claim_by_a_stranger() {
    let mut s = halfway().await;
    let stranger = s.stranger.insecure_clone();
    let ix = client::claim(&ID, &stranger.pubkey(), &s.schedule, &s.mint, &s.stranger_token);
    reject(&mut s, "wrong signer: a stranger claims as if it were the beneficiary", ix, &[&stranger], 0, VestingError::NotBeneficiary)
        .await;
}

#[tokio::test]
async fn wrong_signer_beneficiary_cannot_redirect_to_another_owners_account() {
    let mut s = halfway().await;
    let beneficiary = s.beneficiary.insecure_clone();
    let ix = client::claim(&ID, &beneficiary.pubkey(), &s.schedule, &s.mint, &s.stranger_token);
    reject(
        &mut s,
        "wrong destination: claim into a token account owned by someone else",
        ix,
        &[&beneficiary],
        0,
        VestingError::WrongTokenAccountOwner,
    )
    .await;
}

#[tokio::test]
async fn missing_signer_on_every_signed_instruction() {
    // each instruction is sent with the signer flag cleared and without the signature
    let mut s = halfway().await;
    let new_beneficiary = Keypair::new().pubkey();
    let cases: Vec<(&str, Instruction)> = vec![
        ("missing signer: claim", s.claim_ix()),
        ("missing signer: revoke", s.revoke_ix()),
        ("missing signer: close", s.close_ix()),
        ("missing signer: propose_beneficiary", s.propose_ix(&new_beneficiary)),
        ("missing signer: accept_beneficiary", s.accept_ix(&s.beneficiary.pubkey())),
    ];
    for (name, mut ix) in cases {
        ix.accounts[0].is_signer = false;
        reject_with(&mut s, name, ix, &[], 0, InstructionError::MissingRequiredSignature).await;
    }
}

#[tokio::test]
async fn missing_signer_on_initialize() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let payer = s.env.payer.pubkey();
    // the vault exists, the creator does not sign the initialize
    s.env.send_ok(&[client::create_vault(&payer, &s.schedule, &s.mint)], &[]).await;
    let mut ix = client::initialize_schedule(&ID, &s.params);
    ix.accounts[0].is_signer = false;
    reject_with(
        &mut s,
        "missing signer: initialize_schedule without the creator's signature",
        ix,
        &[],
        0,
        InstructionError::MissingRequiredSignature,
    )
    .await;
    assert!(s.env.account(&s.schedule).await.is_none());
}

// ---------------------------------------------------------------------------------------------------------
// ownership, mints, vault substitution, programs
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn wrong_owner_schedule_account_owned_by_another_program() {
    let mut s = halfway().await;
    patch_schedule(&mut s, |a| a.owner = Pubkey::new_unique()).await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "wrong owner: the schedule account belongs to another program", ix, &[&b], 0, VestingError::WrongAccountOwner).await;
}

#[tokio::test]
async fn wrong_owner_vault_owned_by_the_system_program() {
    let mut s = halfway().await;
    let mut vault = s.env.account(&s.vault).await.unwrap();
    vault.owner = system_program::ID;
    s.env.set_account(&s.vault, vault);
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "wrong owner: the vault is not owned by the token program", ix, &[&b], 0, VestingError::WrongAccountOwner).await;
}

#[tokio::test]
async fn wrong_owner_mint_owned_by_the_system_program() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let fake_mint = Pubkey::new_unique();
    let mut real = s.env.account(&s.mint).await.unwrap();
    real.owner = system_program::ID;
    s.env.set_account(&fake_mint, real);
    s.params.mint = fake_mint;
    s.schedule = s.params.schedule(&ID);
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("wrong owner: the mint is not owned by the token program", result, 1, VestingError::WrongAccountOwner);
}

#[tokio::test]
async fn wrong_mint_on_claim_and_revoke() {
    let mut s = halfway().await;
    let other_mint = s.env.create_mint(6, None).await;
    let b = s.beneficiary.insecure_clone();
    let a = s.authority.insecure_clone();
    let mut claim = s.claim_ix();
    claim.accounts[4] = AccountMeta::new_readonly(other_mint, false);
    reject(&mut s, "wrong mint: claim with another mint account", claim, &[&b], 0, VestingError::WrongMint).await;
    let mut revoke = s.revoke_ix();
    revoke.accounts[5] = AccountMeta::new_readonly(other_mint, false);
    reject(&mut s, "wrong mint: revoke with another mint account", revoke, &[&a], 0, VestingError::WrongMint).await;
}

#[tokio::test]
async fn wrong_mint_source_token_account_of_another_mint() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let other_mint = s.env.create_mint(6, None).await;
    let other_token = s.env.create_loose_token_account(&s.creator.pubkey(), &other_mint).await;
    s.env.mint_to(&other_mint, &other_token, 5_000_000).await;
    s.params.source_token = other_token;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("wrong mint: the funding account holds another mint", result, 1, VestingError::WrongTokenAccountMint);
}

#[tokio::test]
async fn wrong_mint_destination_token_account_of_another_mint() {
    let mut s = halfway().await;
    let other_mint = s.env.create_mint(6, None).await;
    let other_token = s.env.create_token_account(&s.beneficiary.pubkey(), &other_mint).await;
    let b = s.beneficiary.insecure_clone();
    let ix = client::claim(&ID, &b.pubkey(), &s.schedule, &s.mint, &other_token);
    reject(&mut s, "wrong mint: claim into the beneficiary's account of another mint", ix, &[&b], 0, VestingError::WrongTokenAccountMint)
        .await;
}

#[tokio::test]
async fn substituted_vault_a_token_account_of_the_attacker() {
    let mut s = halfway().await;
    let b = s.beneficiary.insecure_clone();
    let mut ix = s.claim_ix();
    ix.accounts[2] = AccountMeta::new(s.stranger_token, false);
    reject(&mut s, "substituted vault: another token account in the vault slot", ix, &[&b], 0, VestingError::WrongVault).await;
}

#[tokio::test]
async fn substituted_vault_a_rogue_token_account_owned_by_the_schedule_address() {
    // an attacker can create a token account whose owner is the schedule address and fill it; it is still not the vault
    let mut s = halfway().await;
    let rogue = s.env.create_loose_token_account(&s.schedule, &s.mint).await;
    let b = s.beneficiary.insecure_clone();
    let mut ix = s.claim_ix();
    ix.accounts[2] = AccountMeta::new(rogue, false);
    reject(&mut s, "substituted vault: a rogue account owned by the schedule address (claim)", ix, &[&b], 0, VestingError::WrongVault)
        .await;

    let mut fresh = Scenario::prepare(Setup::default()).await;
    let rogue = fresh.env.create_loose_token_account(&fresh.schedule, &fresh.mint).await;
    let mut ix = client::initialize_schedule(&ID, &fresh.params);
    ix.accounts[4] = AccountMeta::new(rogue, false);
    let result = init_attack(&mut fresh, ix).await;
    assert_rejected("substituted vault: a rogue account owned by the schedule address (initialize)", result, 1, VestingError::WrongVault);
}

#[tokio::test]
async fn substituted_vault_of_another_schedule() {
    let mut s = halfway().await;
    // a second schedule of the same creator and mint, with its own vault
    let mut second = s.params.clone();
    second.schedule_id = 2;
    let schedule2 = second.schedule(&ID);
    let creator = s.creator.insecure_clone();
    let ixs = [client::create_vault(&creator.pubkey(), &schedule2, &s.mint), client::initialize_schedule(&ID, &second)];
    s.env.send_ok(&ixs, &[&creator]).await;
    // beneficiary of the first schedule names the first schedule but the second schedule's vault
    let b = s.beneficiary.insecure_clone();
    let mut ix = s.claim_ix();
    ix.accounts[2] = AccountMeta::new(client::vault_address(&schedule2, &s.mint), false);
    reject(&mut s, "account substitution: the vault of another schedule", ix, &[&b], 0, VestingError::WrongVault).await;
}

#[tokio::test]
async fn fake_token_program_on_every_instruction() {
    let mut s = halfway().await;
    let (b, a) = (s.beneficiary.insecure_clone(), s.authority.insecure_clone());
    let fake = Pubkey::new_unique();
    let mut claim = s.claim_ix();
    claim.accounts[5] = AccountMeta::new_readonly(fake, false);
    reject(&mut s, "fake token program: a made-up program id (claim)", claim, &[&b], 0, VestingError::WrongTokenProgram).await;
    let mut claim = s.claim_ix();
    claim.accounts[5] = AccountMeta::new_readonly(system_program::ID, false);
    reject(&mut s, "fake token program: the system program in its place (claim)", claim, &[&b], 0, VestingError::WrongTokenProgram).await;
    let mut revoke = s.revoke_ix();
    revoke.accounts[6] = AccountMeta::new_readonly(fake, false);
    reject(&mut s, "fake token program: a made-up program id (revoke)", revoke, &[&a], 0, VestingError::WrongTokenProgram).await;
    let mut close = s.close_ix();
    close.accounts[5] = AccountMeta::new_readonly(fake, false);
    reject(&mut s, "fake token program: a made-up program id (close)", close, &[&a], 0, VestingError::WrongTokenProgram).await;

    let mut fresh = Scenario::prepare(Setup::default()).await;
    let mut init = client::initialize_schedule(&ID, &fresh.params);
    init.accounts[6] = AccountMeta::new_readonly(fake, false);
    let result = init_attack(&mut fresh, init).await;
    assert_rejected("fake token program: a made-up program id (initialize)", result, 1, VestingError::WrongTokenProgram);
}

#[tokio::test]
async fn fake_system_program_on_initialize() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let mut ix = client::initialize_schedule(&ID, &s.params);
    ix.accounts[5] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("fake system program: a made-up program id", result, 1, VestingError::WrongSystemProgram);

    let mut ix = client::initialize_schedule(&ID, &s.params);
    ix.accounts[5] = AccountMeta::new_readonly(spl_token::ID, false);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("fake system program: the token program in its place", result, 1, VestingError::WrongSystemProgram);
    assert!(s.env.account(&s.schedule).await.is_none());
}

#[tokio::test]
async fn token_2022_is_refused_as_program_and_as_mint() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let mut ix = client::initialize_schedule(&ID, &s.params);
    ix.accounts[6] = AccountMeta::new_readonly(TOKEN_2022_ID, false);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("Token-2022: the Token-2022 program in the token program slot", result, 1, VestingError::UnsupportedTokenProgram);

    // a real Token-2022 mint (created by the real Token-2022 program), without any extension
    let mint_2022 = s.env.create_token_2022_mint(6).await;
    s.params.mint = mint_2022;
    s.schedule = s.params.schedule(&ID);
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("Token-2022: a Token-2022 mint with the classic token program", result, 1, VestingError::UnsupportedTokenProgram);
}

// ---------------------------------------------------------------------------------------------------------
// addresses and bumps
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn wrong_pda_a_non_canonical_bump_address_at_initialize() {
    let mut id = 1;
    // find a schedule id whose address has a lower valid bump (almost every id does); mint and creator are fixed by Scenario
    let mut s = Scenario::prepare(Setup::default()).await;
    let (nc, nc_bump) = loop {
        s.params.schedule_id = id;
        if let Some(found) = non_canonical(&s.creator.pubkey(), &s.mint, id) {
            break found;
        }
        id += 1;
    };
    s.schedule = s.params.schedule(&ID);
    let (_, canonical) = client::schedule_address(&ID, &s.creator.pubkey(), &s.mint, id);
    assert!(nc_bump < canonical);
    let mut ix = client::initialize_schedule(&ID, &s.params);
    ix.accounts[1] = AccountMeta::new(nc, false);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("wrong PDA: a valid address with a non-canonical bump as the schedule", result, 1, VestingError::WrongScheduleAddress);
    assert!(s.env.account(&nc).await.is_none());
}

#[tokio::test]
async fn wrong_pda_an_address_of_another_creator_or_id() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let other_creator = Keypair::new().pubkey();
    let others = [
        ("another creator", client::schedule_address(&ID, &other_creator, &s.mint, 1).0),
        ("another id", client::schedule_address(&ID, &s.creator.pubkey(), &s.mint, 99).0),
        ("a random address", Pubkey::new_unique()),
    ];
    for (what, address) in others {
        let mut ix = client::initialize_schedule(&ID, &s.params);
        ix.accounts[1] = AccountMeta::new(address, false);
        let result = init_attack(&mut s, ix).await;
        assert_rejected(&format!("wrong PDA: {what} as the schedule address"), result, 1, VestingError::WrongScheduleAddress);
    }
}

#[tokio::test]
async fn wrong_pda_forged_schedule_at_a_non_canonical_address() {
    // a hostile state no transaction could create: a program-owned copy of the schedule at a non-canonical bump address
    let mut id = 1;
    let mut s = loop {
        let s = Scenario::new(Setup { schedule_id: id, ..Setup::default() }).await;
        if non_canonical(&s.creator.pubkey(), &s.mint, id).is_some() {
            break s;
        }
        id += 1;
    };
    let (nc, nc_bump) = non_canonical(&s.creator.pubkey(), &s.mint, id).unwrap();
    let mut forged = s.env.account(&s.schedule).await.unwrap();
    forged.data[OFFSET_BUMP] = nc_bump;
    s.env.set_account(&nc, forged);
    s.at(500).await;
    let b = s.beneficiary.insecure_clone();
    let ix = client::claim(&ID, &b.pubkey(), &nc, &s.mint, &s.beneficiary_token);
    reject(&mut s, "wrong PDA: a forged schedule copy at a non-canonical bump address", ix, &[&b], 0, VestingError::WrongScheduleAddress)
        .await;
}

#[tokio::test]
async fn wrong_bump_stored_in_the_schedule_does_not_match_the_canonical_bump() {
    let mut id = 1;
    let mut s = loop {
        let s = Scenario::new(Setup { schedule_id: id, ..Setup::default() }).await;
        if non_canonical(&s.creator.pubkey(), &s.mint, id).is_some() {
            break s;
        }
        id += 1;
    };
    let (_, nc_bump) = non_canonical(&s.creator.pubkey(), &s.mint, id).unwrap();
    patch_schedule(&mut s, |a| a.data[OFFSET_BUMP] = nc_bump).await;
    s.at(500).await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "wrong bump: the stored bump is not the canonical bump", ix, &[&b], 0, VestingError::WrongScheduleAddress).await;
}

// ---------------------------------------------------------------------------------------------------------
// initialisation attacks
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn reinitialisation_of_a_live_schedule() {
    let mut s = halfway().await;
    s.at(500).await;
    s.claim().await.unwrap(); // claimed 500_000
                              // the creator tries to create the same schedule again, with a different beneficiary and a reset claimed amount
    s.params.beneficiary = s.stranger.pubkey();
    let before_state = s.state().await;
    let ix = client::initialize_schedule(&ID, &s.params);
    let watched = s.watched();
    let before = s.env.snapshot(&watched).await;
    let result = init_attack(&mut s, ix).await;
    assert_rejected_with("reinitialisation: initialize on a live schedule address", result, 1, InstructionError::AccountAlreadyInitialized);
    s.env.assert_unchanged(&watched, &before).await;
    assert_eq!(s.state().await, before_state);
}

#[tokio::test]
async fn reinitialisation_of_a_program_owned_account_with_zeroed_data() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let rent = s.env.rent().await;
    let zeroed = Account {
        lamports: rent.minimum_balance(client::SCHEDULE_ACCOUNT_SIZE),
        data: vec![0; client::SCHEDULE_ACCOUNT_SIZE],
        owner: ID,
        executable: false,
        rent_epoch: 0,
    };
    s.env.set_account(&s.schedule, zeroed);
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected_with(
        "reinitialisation: initialize over a program-owned zeroed account",
        result,
        1,
        InstructionError::AccountAlreadyInitialized,
    );

    // and such an account is useless for every other instruction
    let b = s.beneficiary.insecure_clone();
    let claim = s.claim_ix();
    let result = s.env.send(&[claim], &[&b]).await;
    assert_rejected_with(
        "uninitialised schedule: claim on a zeroed program-owned account",
        result,
        0,
        InstructionError::InvalidAccountData,
    );
}

#[tokio::test]
async fn truncated_and_padded_schedule_data_is_refused() {
    for (name, len) in [("truncated", client::SCHEDULE_ACCOUNT_SIZE - 1), ("padded", client::SCHEDULE_ACCOUNT_SIZE + 1)] {
        let mut s = halfway().await;
        patch_schedule(&mut s, |a| a.data.resize(len, 0)).await;
        let ix = s.claim_ix();
        let b = s.beneficiary.insecure_clone();
        let result = s.env.send(&[ix], &[&b]).await;
        assert_rejected_with(&format!("layout: {name} schedule data"), result, 0, InstructionError::InvalidAccountData);
    }
}

#[tokio::test]
async fn a_closed_schedule_cannot_be_used_or_revived() {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(2_000).await;
    s.claim().await.unwrap();
    s.close().await.unwrap();
    let b = s.beneficiary.insecure_clone();
    let ix = s.claim_ix();
    let result = s.env.send(&[ix], &[&b]).await;
    assert_rejected("closed account: claim on a closed schedule", result, 0, VestingError::WrongAccountOwner);

    // somebody sends lamports to the closed address to keep it alive: it is still an empty system account
    s.env.airdrop(&s.schedule, 5_000_000).await;
    let account = s.env.account(&s.schedule).await.unwrap();
    assert_eq!(account.owner, system_program::ID);
    assert!(account.data.is_empty());
    let ix = s.claim_ix();
    let result = s.env.send(&[ix], &[&b]).await;
    assert_rejected("closed account: claim on a closed schedule that received lamports", result, 0, VestingError::WrongAccountOwner);
}

#[tokio::test]
async fn corrupt_accounting_in_the_stored_schedule_is_refused() {
    let mut s = halfway().await;
    patch_schedule(&mut s, |a| {
        let total = u64::from_le_bytes(a.data[216..224].try_into().unwrap());
        a.data[OFFSET_CLAIMED..OFFSET_CLAIMED + 8].copy_from_slice(&total.saturating_add(1).to_le_bytes());
    })
    .await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "arithmetic: claimed amount above the total in the stored state", ix, &[&b], 0, VestingError::CorruptSchedule).await;
}

// ---------------------------------------------------------------------------------------------------------
// claim rules
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn double_claim_in_one_transaction() {
    let mut s = halfway().await;
    let b = s.beneficiary.insecure_clone();
    let watched = s.watched();
    let before = s.env.snapshot(&watched).await;
    let result = s.env.send(&[s.claim_ix(), s.claim_ix()], &[&b]).await;
    assert_rejected("double claim: the same claim twice in one transaction", result, 1, VestingError::NothingToClaim);
    // the first claim was rolled back with the transaction
    s.env.assert_unchanged(&watched, &before).await;
    assert_eq!(s.beneficiary_balance().await, 0);

    // the same claim twice in two transactions: the second pays nothing
    s.claim().await.unwrap();
    let ix = s.claim_ix();
    reject(&mut s, "double claim: the same claim again in a later transaction", ix, &[&b], 0, VestingError::NothingToClaim).await;
    assert_eq!(s.beneficiary_balance().await, 500_000);
}

#[tokio::test]
async fn claim_before_the_cliff() {
    let mut s = Scenario::new(Setup { cliff: 500, ..Setup::default() }).await;
    s.at(499).await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "claim before the cliff: one second early", ix, &[&b], 0, VestingError::CliffNotReached).await;
}

#[tokio::test]
async fn claim_before_the_start() {
    let mut s = Scenario::new(Setup { start: 100, cliff: 100, end: 1_100, ..Setup::default() }).await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "claim before the start: the schedule has not begun", ix, &[&b], 0, VestingError::CliffNotReached).await;
}

#[tokio::test]
async fn claim_after_a_revocation() {
    let mut s = halfway().await;
    s.revoke().await.unwrap();
    s.at(2_000).await;
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject(&mut s, "claim after revocation: nothing more is owed", ix, &[&b], 0, VestingError::ScheduleRevoked).await;
}

// ---------------------------------------------------------------------------------------------------------
// revoke rules
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn revoke_by_non_authority() {
    let mut s = halfway().await;
    let stranger = s.stranger.insecure_clone();
    let beneficiary = s.beneficiary.insecure_clone();
    let creator = s.creator.insecure_clone();
    let cases = [
        ("a stranger", &stranger, s.stranger_token),
        ("the beneficiary", &beneficiary, s.beneficiary_token),
        ("the creator (the authority is another key)", &creator, s.creator_token),
    ];
    for (who, key, token) in cases {
        let ix = client::revoke(&ID, &key.pubkey(), &s.schedule, &s.mint, &s.beneficiary_token, &token);
        reject(&mut s, &format!("revoke by non-authority: {who}"), ix, &[key], 0, VestingError::NotAuthority).await;
    }
}

#[tokio::test]
async fn revoke_a_schedule_that_is_not_revocable() {
    let mut s = Scenario::new(Setup { revocable: false, ..Setup::default() }).await;
    s.at(500).await;
    let ix = s.revoke_ix();
    let a = s.authority.insecure_clone();
    reject(&mut s, "revoke non-revocable: the authority tries anyway", ix, &[&a], 0, VestingError::NotRevocable).await;
}

#[tokio::test]
async fn revoke_twice_and_revoke_after_everything_vested() {
    let mut s = halfway().await;
    s.revoke().await.unwrap();
    let ix = s.revoke_ix();
    let a = s.authority.insecure_clone();
    reject(&mut s, "revoke twice: the second revocation", ix, &[&a], 0, VestingError::ScheduleRevoked).await;

    let mut done = Scenario::new(Setup::default()).await;
    done.at(1_000).await;
    let (ix, a) = (done.revoke_ix(), done.authority.insecure_clone());
    reject(&mut done, "revoke after fully vested: there is nothing to take back", ix, &[&a], 0, VestingError::FullyVested).await;
}

#[tokio::test]
async fn revoke_payout_cannot_be_redirected() {
    let mut s = halfway().await;
    let a = s.authority.insecure_clone();
    // the unvested remainder to somebody else's account
    let ix = client::revoke(&ID, &a.pubkey(), &s.schedule, &s.mint, &s.beneficiary_token, &s.stranger_token);
    reject(&mut s, "revoke payout: the remainder redirected to a stranger's account", ix, &[&a], 0, VestingError::WrongTokenAccountOwner)
        .await;
    // the vested part to somebody else's account
    let ix = client::revoke(&ID, &a.pubkey(), &s.schedule, &s.mint, &s.stranger_token, &s.authority_token);
    reject(&mut s, "revoke payout: the vested part redirected to a stranger's account", ix, &[&a], 0, VestingError::WrongTokenAccountOwner)
        .await;
}

// ---------------------------------------------------------------------------------------------------------
// beneficiary change
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn beneficiary_swap_by_a_stranger() {
    let mut s = halfway().await;
    let (stranger, authority, creator) = (s.stranger.insecure_clone(), s.authority.insecure_clone(), s.creator.insecure_clone());
    for (who, key) in [("a stranger", &stranger), ("the authority", &authority), ("the creator", &creator)] {
        let ix = client::propose_beneficiary(&ID, &key.pubkey(), &s.schedule, &stranger.pubkey());
        reject(&mut s, &format!("beneficiary swap: {who} proposes a new beneficiary"), ix, &[key], 0, VestingError::NotBeneficiary).await;
    }
}

#[tokio::test]
async fn beneficiary_swap_acceptance_by_the_wrong_key() {
    let mut s = halfway().await;
    let (beneficiary, stranger) = (s.beneficiary.insecure_clone(), s.stranger.insecure_clone());
    let friend = Keypair::new();

    // accept with nothing pending
    let ix = s.accept_ix(&stranger.pubkey());
    reject(&mut s, "beneficiary swap: accept while no change is pending", ix, &[&stranger], 0, VestingError::NoPendingBeneficiary).await;

    // the beneficiary proposes the friend; the stranger tries to accept in its place
    let propose = s.propose_ix(&friend.pubkey());
    s.env.send_ok(&[propose], &[&beneficiary]).await;
    let ix = s.accept_ix(&stranger.pubkey());
    reject(
        &mut s,
        "beneficiary swap: a stranger accepts a proposal made for another key",
        ix,
        &[&stranger],
        0,
        VestingError::NotPendingBeneficiary,
    )
    .await;

    // the proposed key has not accepted: it cannot claim yet
    let ix = client::claim(&ID, &friend.pubkey(), &s.schedule, &s.mint, &s.stranger_token);
    reject(&mut s, "beneficiary swap: the proposed key claims before accepting", ix, &[&friend], 0, VestingError::NotBeneficiary).await;
}

#[tokio::test]
async fn beneficiary_swap_to_invalid_keys() {
    let mut s = halfway().await;
    let beneficiary = s.beneficiary.insecure_clone();
    for (what, key) in [("the default key", Pubkey::default()), ("the current beneficiary", beneficiary.pubkey())] {
        let ix = s.propose_ix(&key);
        reject(&mut s, &format!("beneficiary swap: propose {what}"), ix, &[&beneficiary], 0, VestingError::InvalidParty).await;
    }
}

#[tokio::test]
async fn initialize_with_the_default_key_as_beneficiary_or_authority() {
    let mut s = Scenario::prepare(Setup::default()).await;
    s.params.beneficiary = Pubkey::default();
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("invalid party: the default key as beneficiary", result, 1, VestingError::InvalidParty);
    s.params.beneficiary = s.beneficiary.pubkey();
    s.params.authority = Pubkey::default();
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("invalid party: the default key as authority", result, 1, VestingError::InvalidParty);
}

// ---------------------------------------------------------------------------------------------------------
// duplicate accounts
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_accounts_on_initialize() {
    let mut s = Scenario::prepare(Setup::default()).await;
    let vault = s.vault;
    let cases: Vec<(&str, usize, Pubkey)> = vec![
        ("the vault as the funding account", 3, vault),
        ("the schedule as the vault", 4, s.schedule),
        ("the creator wallet as the funding account", 3, s.creator.pubkey()),
        ("the mint as the funding account", 3, s.mint),
    ];
    for (what, slot, key) in cases {
        let mut ix = client::initialize_schedule(&ID, &s.params);
        ix.accounts[slot] = AccountMeta::new(key, false);
        let result = init_attack(&mut s, ix).await;
        assert_rejected(&format!("duplicate accounts: initialize with {what}"), result, 1, VestingError::DuplicateAccount);
    }
}

#[tokio::test]
async fn duplicate_accounts_on_claim_revoke_and_close() {
    let mut s = halfway().await;
    let (b, a) = (s.beneficiary.insecure_clone(), s.authority.insecure_clone());
    let (vault, schedule) = (s.vault, s.schedule);

    let mut ix = s.claim_ix();
    ix.accounts[3] = AccountMeta::new(vault, false);
    reject(&mut s, "duplicate accounts: claim with the vault as the destination", ix, &[&b], 0, VestingError::DuplicateAccount).await;
    let mut ix = s.claim_ix();
    ix.accounts[2] = AccountMeta::new(schedule, false);
    reject(&mut s, "duplicate accounts: claim with the schedule as the vault", ix, &[&b], 0, VestingError::DuplicateAccount).await;

    let mut ix = s.revoke_ix();
    ix.accounts[3] = AccountMeta::new(vault, false);
    reject(&mut s, "duplicate accounts: revoke with the vault as the beneficiary's account", ix, &[&a], 0, VestingError::DuplicateAccount)
        .await;
    let mut ix = s.revoke_ix();
    ix.accounts[4] = AccountMeta::new(vault, false);
    reject(&mut s, "duplicate accounts: revoke with the vault as the authority's account", ix, &[&a], 0, VestingError::DuplicateAccount)
        .await;
    let mut ix = s.revoke_ix();
    ix.accounts[3] = AccountMeta::new(schedule, false);
    reject(
        &mut s,
        "duplicate accounts: revoke with the schedule as the beneficiary's account",
        ix,
        &[&a],
        0,
        VestingError::DuplicateAccount,
    )
    .await;

    let mut ix = s.close_ix();
    ix.accounts[3] = AccountMeta::new(vault, false);
    reject(&mut s, "duplicate accounts: close with the vault as the authority's account", ix, &[&a], 0, VestingError::DuplicateAccount)
        .await;
}

// ---------------------------------------------------------------------------------------------------------
// amounts and arithmetic
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn overflow_amounts_on_initialize() {
    // more than the creator holds
    let mut s = Scenario::prepare(Setup::default()).await;
    s.params.total_amount = u64::MAX;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("overflow amounts: a total of u64::MAX that the creator does not hold", result, 1, VestingError::InsufficientFunds);

    // one token more than the creator holds
    s.params.total_amount = 5_000_001;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("overflow amounts: one token more than the creator holds", result, 1, VestingError::InsufficientFunds);

    // timestamps at the limits of i64
    s.params.total_amount = 1;
    s.params.start_ts = i64::MIN;
    s.params.cliff_ts = i64::MIN;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("overflow amounts: a start of i64::MIN", result, 1, VestingError::NegativeTimestamp);
    s.params.start_ts = T0;
    s.params.cliff_ts = T0;
    s.params.end_ts = i64::MAX;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected("overflow amounts: an end of i64::MAX", result, 1, VestingError::DurationTooLong);
    assert!(s.env.account(&s.schedule).await.is_none());
}

#[tokio::test]
async fn funding_account_of_somebody_else() {
    let mut s = Scenario::prepare(Setup::default()).await;
    s.env.mint_to(&s.mint, &s.stranger_token, 5_000_000).await;
    s.params.source_token = s.stranger_token;
    let ix = client::initialize_schedule(&ID, &s.params);
    let result = init_attack(&mut s, ix).await;
    assert_rejected(
        "funding: the creator names another owner's token account as the source",
        result,
        1,
        VestingError::WrongTokenAccountOwner,
    );
}

// ---------------------------------------------------------------------------------------------------------
// closing
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn closing_with_a_remaining_balance() {
    let mut s = halfway().await;
    let a = s.authority.insecure_clone();
    let ix = s.close_ix();
    reject(&mut s, "closing with a remaining balance: halfway through the schedule", ix, &[&a], 0, VestingError::NotSettled).await;

    s.claim().await.unwrap();
    let ix = s.close_ix();
    reject(&mut s, "closing with a remaining balance: after a partial claim", ix, &[&a], 0, VestingError::NotSettled).await;

    // everything has vested but the beneficiary has not collected: the authority cannot take the beneficiary's tokens
    s.at(5_000).await;
    let ix = s.close_ix();
    reject(
        &mut s,
        "closing with a remaining balance: vested but unclaimed tokens are still in the vault",
        ix,
        &[&a],
        0,
        VestingError::NotSettled,
    )
    .await;
    assert_eq!(s.vault_balance().await, 500_000);
}

#[tokio::test]
async fn closing_by_a_non_authority() {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(2_000).await;
    s.claim().await.unwrap();
    let (stranger, beneficiary) = (s.stranger.insecure_clone(), s.beneficiary.insecure_clone());
    let ix = client::close(&ID, &stranger.pubkey(), &s.schedule, &s.mint, &s.stranger_token);
    reject(&mut s, "close by non-authority: a stranger", ix, &[&stranger], 0, VestingError::NotAuthority).await;
    let ix = client::close(&ID, &beneficiary.pubkey(), &s.schedule, &s.mint, &s.beneficiary_token);
    reject(&mut s, "close by non-authority: the beneficiary", ix, &[&beneficiary], 0, VestingError::NotAuthority).await;
}

// ---------------------------------------------------------------------------------------------------------
// clock and instruction data
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn clock_manipulation_through_instruction_data() {
    let mut s = Scenario::new(Setup { cliff: 500, ..Setup::default() }).await;
    s.at(10).await; // far before the cliff
    let b = s.beneficiary.insecure_clone();

    // a time smuggled in after the instruction tag, as a fake "now" far in the future
    let mut ix = s.claim_ix();
    ix.data.extend_from_slice(&(T0 + 10 * DAY).to_le_bytes());
    reject_with(
        &mut s,
        "clock manipulation: a timestamp appended to the claim data",
        ix,
        &[&b],
        0,
        InstructionError::InvalidInstructionData,
    )
    .await;

    // the same without the tag: data made only of a timestamp
    let mut ix = s.claim_ix();
    ix.data = (T0 + 10 * DAY).to_le_bytes().to_vec();
    reject_with(&mut s, "clock manipulation: claim data that is only a timestamp", ix, &[&b], 0, InstructionError::InvalidInstructionData)
        .await;

    // the claim itself still follows the sysvar clock
    let ix = s.claim_ix();
    reject(&mut s, "clock manipulation: the plain claim at that time is early", ix, &[&b], 0, VestingError::CliffNotReached).await;
}

#[tokio::test]
async fn clock_manipulation_through_a_forged_clock_account() {
    let mut s = Scenario::new(Setup { cliff: 500, ..Setup::default() }).await;
    s.at(10).await;
    let b = s.beneficiary.insecure_clone();

    // an account that holds a Clock far in the future, owned by the sysvar program, at an address of the attacker's choosing
    let mut clock: Clock = s.env.sysvar().await;
    clock.unix_timestamp = T0 + 10 * DAY;
    let forged = Pubkey::new_unique();
    let account = Account::new_data(1_000_000, &clock, &solana_sdk::sysvar::id()).unwrap();
    s.env.set_account(&forged, account);

    // appended as an extra account
    let mut ix = s.claim_ix();
    ix.accounts.push(AccountMeta::new_readonly(forged, false));
    reject(&mut s, "clock manipulation: a forged clock account appended to the claim", ix, &[&b], 0, VestingError::UnexpectedAccounts)
        .await;

    // in the place of an expected account
    let mut ix = s.claim_ix();
    ix.accounts[5] = AccountMeta::new_readonly(forged, false);
    reject(&mut s, "clock manipulation: a forged clock account in the token program slot", ix, &[&b], 0, VestingError::WrongTokenProgram)
        .await;

    // the real clock sysvar as an extra account is refused too: the program takes no sysvar accounts
    let mut ix = s.claim_ix();
    ix.accounts.push(AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false));
    reject(&mut s, "clock manipulation: the clock sysvar passed as an account", ix, &[&b], 0, VestingError::UnexpectedAccounts).await;
}

#[tokio::test]
async fn malformed_instruction_data_and_account_lists() {
    let mut s = halfway().await;
    let b = s.beneficiary.insecure_clone();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty data", vec![]),
        ("unknown tag", vec![9]),
        ("claim with a trailing byte", vec![1, 0]),
        ("revoke with a trailing byte", vec![2, 0]),
        ("initialize with no fields", vec![0]),
    ];
    for (what, data) in cases {
        let mut ix = s.claim_ix();
        ix.data = data;
        reject_with(&mut s, &format!("malformed data: {what}"), ix, &[&b], 0, InstructionError::InvalidInstructionData).await;
    }
    // a revocable flag that is neither 0 nor 1, and data one byte short
    let mut fresh = Scenario::prepare(Setup::default()).await;
    let mut init = client::initialize_schedule(&ID, &fresh.params);
    let last = init.data.len() - 1;
    init.data[last] = 2;
    let result = init_attack(&mut fresh, init).await;
    assert_rejected_with("malformed data: a revocable flag of 2", result, 1, InstructionError::InvalidInstructionData);
    let mut init = client::initialize_schedule(&ID, &fresh.params);
    init.data.pop();
    let result = init_attack(&mut fresh, init).await;
    assert_rejected_with("malformed data: initialize one byte short", result, 1, InstructionError::InvalidInstructionData);

    // account lists: one too many, one too few
    let mut ix = s.claim_ix();
    ix.accounts.push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
    reject(&mut s, "account list: an extra account on claim", ix, &[&b], 0, VestingError::UnexpectedAccounts).await;
    let mut ix = s.claim_ix();
    ix.accounts.pop();
    reject_with(&mut s, "account list: a missing account on claim", ix, &[&b], 0, InstructionError::NotEnoughAccountKeys).await;
}

#[tokio::test]
async fn read_only_accounts_that_must_be_written() {
    let mut s = halfway().await;
    let (b, a) = (s.beneficiary.insecure_clone(), s.authority.insecure_clone());
    for (what, slot) in [("the schedule", 1usize), ("the vault", 2), ("the destination", 3)] {
        let mut ix = s.claim_ix();
        ix.accounts[slot].is_writable = false;
        reject(&mut s, &format!("read-only account: claim with {what} read-only"), ix, &[&b], 0, VestingError::AccountNotWritable).await;
    }
    let mut ix = s.close_ix();
    ix.accounts[0].is_writable = false;
    reject(&mut s, "read-only account: close with the rent receiver read-only", ix, &[&a], 0, VestingError::AccountNotWritable).await;
}

// ---------------------------------------------------------------------------------------------------------
// token account state
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn frozen_accounts() {
    let mut s = Scenario::new(Setup { freeze_authority: true, ..Setup::default() }).await;
    s.at(500).await;
    let b = s.beneficiary.insecure_clone();

    s.env.freeze(&s.beneficiary_token, &s.mint).await;
    let ix = s.claim_ix();
    reject(&mut s, "frozen account: claim into a frozen destination", ix, &[&b], 0, VestingError::TokenAccountFrozen).await;

    let mut s = Scenario::new(Setup { freeze_authority: true, ..Setup::default() }).await;
    s.at(500).await;
    s.env.freeze(&s.vault, &s.mint).await;
    let (ix, b) = (s.claim_ix(), s.beneficiary.insecure_clone());
    reject(&mut s, "frozen account: claim from a frozen vault", ix, &[&b], 0, VestingError::TokenAccountFrozen).await;

    let mut fresh = Scenario::prepare(Setup { freeze_authority: true, ..Setup::default() }).await;
    fresh.env.freeze(&fresh.creator_token, &fresh.mint).await;
    let ix = client::initialize_schedule(&ID, &fresh.params);
    let result = init_attack(&mut fresh, ix).await;
    assert_rejected("frozen account: a frozen funding account", result, 1, VestingError::TokenAccountFrozen);
}

#[tokio::test]
async fn a_vault_with_a_delegate_or_a_close_authority_is_refused() {
    for (name, delegate) in [("a delegate", true), ("a close authority", false)] {
        let mut s = halfway().await;
        let mut state = TokenAccountState::unpack(&s.env.account(&s.vault).await.unwrap().data).unwrap();
        if delegate {
            state.delegate = Some(s.stranger.pubkey()).into();
            state.delegated_amount = 1_000_000;
        } else {
            state.close_authority = Some(s.stranger.pubkey()).into();
        }
        let mut account = token_account_to_account(&state);
        account.lamports = s.env.lamports(&s.vault).await;
        s.env.set_account(&s.vault, account);
        let (b, a) = (s.beneficiary.insecure_clone(), s.authority.insecure_clone());
        let ix = s.claim_ix();
        reject(&mut s, &format!("unsafe vault: claim from a vault that has {name}"), ix, &[&b], 0, VestingError::UnsafeVaultAuthority)
            .await;
        let ix = s.revoke_ix();
        reject(&mut s, &format!("unsafe vault: revoke on a vault that has {name}"), ix, &[&a], 0, VestingError::UnsafeVaultAuthority).await;
    }
}

#[tokio::test]
async fn an_uninitialised_vault_is_refused() {
    let mut s = halfway().await;
    let mut account = s.env.account(&s.vault).await.unwrap();
    let state = TokenAccountState { state: AccountState::Uninitialized, ..TokenAccountState::unpack(&account.data).unwrap() };
    TokenAccountState::pack(state, &mut account.data).unwrap();
    s.env.set_account(&s.vault, account);
    let ix = s.claim_ix();
    let b = s.beneficiary.insecure_clone();
    reject_with(
        &mut s,
        "uninitialised vault: the token account state is Uninitialized",
        ix,
        &[&b],
        0,
        InstructionError::UninitializedAccount,
    )
    .await;
}

// ---------------------------------------------------------------------------------------------------------
// attacks that need no program bug: what the runtime and the token program refuse
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn direct_withdrawal_from_the_vault_by_an_outsider() {
    let mut s = halfway().await;
    let stranger = s.stranger.insecure_clone();
    // the attacker signs as the owner of the vault: it is not the owner
    let ix = spl_token::instruction::transfer(&spl_token::ID, &s.vault, &s.stranger_token, &stranger.pubkey(), &[], 1).unwrap();
    reject_by(
        &mut s,
        "direct withdrawal: the attacker signs as the vault's owner",
        ix,
        &[&stranger],
        0,
        &spl_token::ID,
        token_error(TokenError::OwnerMismatch),
    )
    .await;
    // naming the schedule address as the authority without its signature
    let mut ix = spl_token::instruction::transfer(&spl_token::ID, &s.vault, &s.stranger_token, &s.schedule, &[], 1).unwrap();
    ix.accounts[2].is_signer = false;
    reject_by(
        &mut s,
        "direct withdrawal: the schedule address as an unsigned authority",
        ix,
        &[],
        0,
        &spl_token::ID,
        InstructionError::MissingRequiredSignature,
    )
    .await;
    // taking over the vault by changing its owner
    let ix = spl_token::instruction::set_authority(
        &spl_token::ID,
        &s.vault,
        Some(&stranger.pubkey()),
        spl_token::instruction::AuthorityType::AccountOwner,
        &stranger.pubkey(),
        &[],
    )
    .unwrap();
    reject_by(
        &mut s,
        "direct withdrawal: set_authority on the vault by the attacker",
        ix,
        &[&stranger],
        0,
        &spl_token::ID,
        token_error(TokenError::OwnerMismatch),
    )
    .await;
}

#[tokio::test]
async fn lamports_cannot_be_drained_from_the_schedule_account() {
    let mut s = halfway().await;
    let stranger = s.stranger.insecure_clone();
    // a system transfer or assign that names the program-owned schedule account as its source can only be unsigned
    let mut transfer = system_instruction::transfer(&s.schedule, &stranger.pubkey(), 1);
    transfer.accounts[0].is_signer = false;
    reject_by(
        &mut s,
        "lamport drain: a system transfer out of the schedule account",
        transfer,
        &[],
        0,
        &system_program::ID,
        InstructionError::MissingRequiredSignature,
    )
    .await;
    let mut assign = system_instruction::assign(&s.schedule, &stranger.pubkey());
    assign.accounts[0].is_signer = false;
    reject_by(
        &mut s,
        "lamport drain: a system assign of the schedule account",
        assign,
        &[],
        0,
        &system_program::ID,
        InstructionError::MissingRequiredSignature,
    )
    .await;
    let schedule = s.schedule;
    assert!(s.env.lamports(&schedule).await > 0);
}
