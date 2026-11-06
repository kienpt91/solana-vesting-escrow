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
use spl_token::state::{Account as TokenAccountState, AccountState};
use vesting_client::{self as client, VestingError, ID};
use vesting_escrow::pda::SCHEDULE_SEED;
use vesting_tests::attack::{assert_rejected, assert_rejected_with};
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
