//! The program doing what it is for: every instruction on its intended path, asserted on token balances and on the
//! recorded schedule.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use solana_program::program_pack::Pack;
use solana_sdk::{signature::Keypair, signer::Signer};
use spl_token::state::Account as TokenAccountState;
use vesting_client as client;
use vesting_tests::{token_account_to_account, Env, Scenario, Setup, T0};

/// 1_000_000 tokens vest linearly over 1_000 seconds from T0, no cliff.
fn linear() -> Setup {
    Setup::default()
}

#[tokio::test]
async fn initialize_funds_the_vault_and_records_the_schedule() {
    let mut s = Scenario::new(linear()).await;

    assert_eq!(s.vault_balance().await, 1_000_000, "the vault holds the total");
    assert_eq!(s.env.balance(&s.creator_token).await, 4_000_000, "the creator paid the total");

    let state = s.state().await;
    assert_eq!(state.creator, s.creator.pubkey());
    assert_eq!(state.authority, s.authority.pubkey());
    assert_eq!(state.beneficiary, s.beneficiary.pubkey());
    assert!(!state.has_pending_beneficiary());
    assert_eq!(state.mint, s.mint);
    assert_eq!(state.vault, s.vault);
    assert_eq!((state.total_amount, state.claimed_amount, state.returned_amount), (1_000_000, 0, 0));
    assert_eq!((state.start_ts, state.cliff_ts, state.end_ts), (T0, T0, T0 + 1_000));
    assert!(state.revocable && !state.revoked);
    assert_eq!(state.id, 1);
    let (address, bump) = client::schedule_address(&s.program_id(), &s.creator.pubkey(), &s.mint, 1);
    assert_eq!(address, s.schedule);
    assert_eq!(state.bump, bump, "the canonical bump is stored");

    // the schedule account is owned by the program, exactly as large as the layout and rent exempt
    let account = s.env.account(&s.schedule).await.unwrap();
    assert_eq!(account.owner, s.program_id());
    assert_eq!(account.data.len(), client::SCHEDULE_ACCOUNT_SIZE);
    let rent = s.env.ctx.banks_client.get_rent().await.unwrap();
    assert!(rent.is_exempt(account.lamports, account.data.len()));

    // the vault is a token account owned by the schedule address
    let vault = TokenAccountState::unpack(&s.env.account(&s.vault).await.unwrap().data).unwrap();
    assert_eq!(vault.owner, s.schedule);
    assert_eq!(vault.mint, s.mint);
}

#[tokio::test]
async fn claim_pays_the_linear_amount_and_repeated_claims_add_up_to_the_total() {
    let mut s = Scenario::new(linear()).await;

    s.at(250).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 250_000);
    assert_eq!(s.state().await.claimed_amount, 250_000);

    s.at(600).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 600_000, "the second claim pays only the difference");

    s.at(5_000).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1_000_000, "after the end the whole total has been paid");
    assert_eq!(s.vault_balance().await, 0);
    assert_eq!(s.state().await.claimed_amount, 1_000_000);
}

#[tokio::test]
async fn a_cliff_holds_everything_back_and_then_releases_the_accrued_amount() {
    // cliff after a quarter of the duration
    let mut s = Scenario::new(Setup { cliff: 250, ..linear() }).await;

    s.at(249).await;
    assert!(s.claim().await.is_err());
    assert_eq!(s.beneficiary_balance().await, 0);

    s.at(250).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 250_000, "at the cliff the linear amount accrued since the start is paid");

    s.at(500).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 500_000);
}

#[tokio::test]
async fn revoke_pays_the_vested_part_to_the_beneficiary_and_returns_the_rest() {
    let mut s = Scenario::new(linear()).await;

    s.at(300).await;
    s.claim().await.unwrap(); // 300_000 claimed
    s.at(400).await;
    s.revoke().await.unwrap();

    assert_eq!(s.beneficiary_balance().await, 400_000, "vested 400_000 in total: 300_000 earlier plus 100_000 now");
    assert_eq!(s.authority_balance().await, 600_000, "the unvested remainder returns to the authority");
    assert_eq!(s.vault_balance().await, 0);
    let state = s.state().await;
    assert!(state.revoked);
    assert_eq!(state.revoked_at, T0 + 400);
    assert_eq!((state.claimed_amount, state.returned_amount), (400_000, 600_000));
}

#[tokio::test]
async fn revoke_before_the_cliff_returns_everything() {
    let mut s = Scenario::new(Setup { cliff: 500, ..linear() }).await;
    s.at(100).await;
    s.revoke().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 0);
    assert_eq!(s.authority_balance().await, 1_000_000);
}

#[tokio::test]
async fn close_after_completion_returns_the_rent_and_removes_both_accounts() {
    let mut s = Scenario::new(linear()).await;
    s.at(2_000).await;
    s.claim().await.unwrap();

    let schedule_lamports = s.env.lamports(&s.schedule).await;
    let vault_lamports = s.env.lamports(&s.vault).await;
    let authority_before = s.env.lamports(&s.authority.pubkey()).await;
    assert!(schedule_lamports > 0 && vault_lamports > 0);

    s.close().await.unwrap();

    let authority_after = s.env.lamports(&s.authority.pubkey()).await;
    assert_eq!(
        authority_after - authority_before,
        schedule_lamports + vault_lamports,
        "both rent deposits go to the authority (the payer of the fee is another key)"
    );
    assert!(s.env.account(&s.schedule).await.is_none(), "the schedule account is gone");
    assert!(s.env.account(&s.vault).await.is_none(), "the vault is gone");
}

#[tokio::test]
async fn close_after_a_revocation() {
    let mut s = Scenario::new(linear()).await;
    s.at(100).await;
    s.revoke().await.unwrap();
    s.close().await.unwrap();
    assert!(s.env.account(&s.schedule).await.is_none());
    assert!(s.env.account(&s.vault).await.is_none());
}

#[tokio::test]
async fn the_beneficiary_changes_in_two_steps_and_only_the_new_key_claims() {
    let mut s = Scenario::new(linear()).await;
    let new_beneficiary = Keypair::new();
    s.env.airdrop(&new_beneficiary.pubkey(), 1_000_000_000).await;
    let new_token = s.env.create_token_account(&new_beneficiary.pubkey(), &s.mint).await;

    s.at(100).await;
    s.claim().await.unwrap(); // the old beneficiary claims 100_000

    // step 1: the current beneficiary proposes
    let propose = s.propose_ix(&new_beneficiary.pubkey());
    let old = s.beneficiary.insecure_clone();
    s.env.send_ok(&[propose], &[&old]).await;
    let state = s.state().await;
    assert_eq!(state.beneficiary, s.beneficiary.pubkey(), "nothing changes until the new key accepts");
    assert_eq!(state.pending_beneficiary, new_beneficiary.pubkey());

    // step 2: the proposed key accepts
    let accept = s.accept_ix(&new_beneficiary.pubkey());
    s.env.send_ok(&[accept], &[&new_beneficiary]).await;
    let state = s.state().await;
    assert_eq!(state.beneficiary, new_beneficiary.pubkey());
    assert!(!state.has_pending_beneficiary());

    // the new beneficiary claims what has vested since, into its own account
    s.at(500).await;
    let claim = client::claim(&s.program_id(), &new_beneficiary.pubkey(), &s.schedule, &s.mint, &new_token);
    s.env.send_ok(&[claim], &[&new_beneficiary]).await;
    assert_eq!(s.env.balance(&new_token).await, 400_000, "vested 500_000 minus the 100_000 the old beneficiary took");

    // the old beneficiary no longer can
    s.at(600).await;
    assert!(s.claim().await.is_err());
}

#[tokio::test]
async fn a_new_proposal_replaces_a_pending_one() {
    let mut s = Scenario::new(linear()).await;
    let (first, second) = (Keypair::new(), Keypair::new());
    let old = s.beneficiary.insecure_clone();
    let (a, b) = (s.propose_ix(&first.pubkey()), s.propose_ix(&second.pubkey()));
    s.env.send_ok(&[a], &[&old]).await;
    s.env.send_ok(&[b], &[&old]).await;
    assert_eq!(s.state().await.pending_beneficiary, second.pubkey());
}

#[tokio::test]
async fn the_authority_may_be_the_creator() {
    let mut s = Scenario::new(Setup { separate_authority: false, ..linear() }).await;
    assert_eq!(s.params.authority, s.creator.pubkey());
    s.at(500).await;
    let before = s.authority_balance().await;
    s.revoke().await.unwrap();
    assert_eq!(s.authority_balance().await - before, 500_000);
}

#[tokio::test]
async fn the_authority_and_the_beneficiary_may_be_one_wallet() {
    // the same wallet is both: a revocation then pays one token account twice
    let mut s = Scenario::prepare(linear()).await;
    s.params.authority = s.beneficiary.pubkey();
    s.authority = s.beneficiary.insecure_clone();
    s.authority_token = s.beneficiary_token;
    s.initialize().await.unwrap();
    s.at(250).await;
    s.revoke().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1_000_000, "250_000 vested plus 750_000 returned, to the same account");
}

#[tokio::test]
async fn schedules_of_one_creator_do_not_interfere() {
    let mut s = Scenario::new(linear()).await;
    // a second schedule with another id, a smaller total, another end
    let mut second = s.params.clone();
    second.schedule_id = 2;
    second.total_amount = 300;
    second.end_ts = T0 + 100;
    let schedule2 = second.schedule(&s.program_id());
    assert_ne!(schedule2, s.schedule);
    let creator = s.creator.insecure_clone();
    let ixs = [client::create_vault(&creator.pubkey(), &schedule2, &s.mint), client::initialize_schedule(&s.program_id(), &second)];
    s.env.send_ok(&ixs, &[&creator]).await;

    s.at(100).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 100_000);
    let beneficiary = s.beneficiary.insecure_clone();
    let claim2 = client::claim(&s.program_id(), &beneficiary.pubkey(), &schedule2, &s.mint, &s.beneficiary_token);
    s.env.send_ok(&[claim2], &[&beneficiary]).await;
    assert_eq!(s.beneficiary_balance().await, 100_300);
    assert_eq!(s.vault_balance().await, 900_000, "the first vault is untouched by the second schedule");
}

#[tokio::test]
async fn a_back_dated_schedule_has_part_of_its_amount_vested_at_creation() {
    // started 600 s ago, ends in 400 s
    let mut s = Scenario::new(Setup { start: -600, cliff: -600, end: 400, ..linear() }).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 600_000);
}

#[tokio::test]
async fn a_donation_to_the_vault_is_swept_to_the_authority_on_close() {
    let mut s = Scenario::new(linear()).await;
    // a stranger sends 7 tokens to the vault
    let donor = s.stranger.insecure_clone();
    s.env.mint_to(&s.mint, &s.stranger_token, 7).await;
    s.env.transfer_tokens(&s.stranger_token, &s.vault, &donor, 7).await;
    assert_eq!(s.vault_balance().await, 1_000_007);

    s.at(2_000).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1_000_000, "the donation does not change what vests");
    s.close().await.unwrap();
    assert_eq!(s.authority_balance().await, 7, "the surplus goes to the authority and the vault closes");
    assert!(s.env.account(&s.vault).await.is_none());
}

#[tokio::test]
async fn lamports_sent_to_the_schedule_address_first_do_not_block_creation() {
    let mut s = Scenario::prepare(linear()).await;
    // griefing attempt: lamports on the address make `create_account` fail for everyone (the runtime only accepts
    // a first deposit that is rent exempt for an empty account)
    let rent = s.env.ctx.banks_client.get_rent().await.unwrap();
    let dust = rent.minimum_balance(0);
    s.env.airdrop(&s.schedule, dust).await;
    assert_eq!(s.env.lamports(&s.schedule).await, dust);
    s.initialize().await.unwrap();
    let account = s.env.account(&s.schedule).await.unwrap();
    assert_eq!(account.owner, s.program_id());
    let rent = s.env.ctx.banks_client.get_rent().await.unwrap();
    assert!(rent.is_exempt(account.lamports, account.data.len()));
}

#[tokio::test]
async fn an_oversized_donation_to_the_schedule_address_is_kept_not_refunded() {
    let mut s = Scenario::prepare(linear()).await;
    let rent = s.env.ctx.banks_client.get_rent().await.unwrap();
    let donation = rent.minimum_balance(client::SCHEDULE_ACCOUNT_SIZE) * 3;
    s.env.airdrop(&s.schedule, donation).await;
    s.initialize().await.unwrap();
    assert_eq!(s.env.lamports(&s.schedule).await, donation, "no top-up was needed and none is taken back");
}

#[tokio::test]
async fn a_destination_with_a_delegate_and_a_close_authority_can_still_be_paid() {
    let mut s = Scenario::new(linear()).await;
    // the beneficiary's token account has a delegate and a close authority: they can only touch that account
    let mut state = TokenAccountState::unpack(&s.env.account(&s.beneficiary_token).await.unwrap().data).unwrap();
    state.delegate = Some(s.stranger.pubkey()).into();
    state.delegated_amount = 5;
    state.close_authority = Some(s.stranger.pubkey()).into();
    s.env.set_account(&s.beneficiary_token, token_account_to_account(&state));
    s.at(500).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 500_000);
}

#[tokio::test]
async fn an_address_can_be_used_again_after_the_schedule_is_closed() {
    let mut s = Scenario::new(linear()).await;
    s.at(2_000).await;
    s.claim().await.unwrap();
    s.close().await.unwrap();
    assert!(s.env.account(&s.schedule).await.is_none());

    // same creator, mint and id: a fresh schedule, created from scratch
    s.params.start_ts = T0 + 2_000;
    s.params.cliff_ts = T0 + 2_000;
    s.params.end_ts = T0 + 3_000;
    s.params.total_amount = 123;
    s.initialize().await.unwrap();
    let state = s.state().await;
    assert_eq!((state.total_amount, state.claimed_amount, state.start_ts), (123, 0, T0 + 2_000));
}

#[tokio::test]
async fn the_client_status_agrees_with_what_the_chain_pays() {
    let mut s = Scenario::new(Setup { cliff: 100, ..linear() }).await;
    let mut paid = 0;
    for offset in [50, 100, 101, 333, 999, 1_000, 1_500] {
        s.at(offset).await;
        let before = s.state().await;
        let status = client::status(&before, T0 + offset).unwrap();
        let result = s.claim().await;
        if status.claimable == 0 {
            assert!(result.is_err(), "nothing claimable at {offset}");
        } else {
            result.unwrap();
            paid += status.claimable;
        }
        assert_eq!(s.beneficiary_balance().await, paid, "at {offset}");
        assert_eq!(s.state().await.claimed_amount, paid);
    }
    assert_eq!(paid, 1_000_000);
}

#[tokio::test]
async fn the_token_programs_are_sbf_binaries_and_the_vesting_program_is_a_native_builtin() {
    // pins what "in process" means in this repository (README, "What the tests execute")
    let mut env = Env::new().await;
    let loaders = [solana_sdk::bpf_loader::id(), solana_sdk::bpf_loader_deprecated::id(), solana_sdk::bpf_loader_upgradeable::id()];
    for (name, id) in [
        ("token", spl_token::ID),
        ("token-2022", vesting_tests::TOKEN_2022_ID),
        ("associated token account", vesting_client::ASSOCIATED_TOKEN_PROGRAM_ID),
    ] {
        let account = env.account(&id).await.unwrap_or_else(|| panic!("{name} program is loaded"));
        assert!(account.executable, "{name}");
        assert!(loaders.contains(&account.owner), "{name} program must be owned by a BPF loader, found {}", account.owner);
    }
    let ours = env.account(&vesting_escrow::ID).await.expect("vesting program is loaded");
    assert!(ours.executable);
    assert_eq!(ours.owner, solana_sdk::native_loader::id(), "the vesting program is registered as a native builtin");
}

#[tokio::test]
async fn a_revocation_clears_a_pending_beneficiary_change() {
    let mut s = Scenario::new(linear()).await;
    let successor = Keypair::new();
    let (propose, old) = (s.propose_ix(&successor.pubkey()), s.beneficiary.insecure_clone());
    s.env.send_ok(&[propose], &[&old]).await;
    assert!(s.state().await.has_pending_beneficiary());
    s.at(300).await;
    s.revoke().await.unwrap();
    let state = s.state().await;
    assert!(!state.has_pending_beneficiary(), "a revoked schedule carries no open proposal");
    assert_eq!(state.beneficiary, s.beneficiary.pubkey());
    // the proposal cannot be accepted any more
    let accept = s.accept_ix(&successor.pubkey());
    let result = s.env.send(&[accept], &[&successor]).await;
    assert_eq!(result, Err(vesting_tests::expect(0, vesting_client::VestingError::NoPendingBeneficiary)));
}
