//! The instruction builders: account order, signer and writable flags, and data. These are the contract between the
//! client and `docs/accounts.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use solana_program::{pubkey::Pubkey, system_program};
use vesting_client::*;

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

fn params() -> InitializeParams {
    InitializeParams {
        creator: key(1),
        schedule_id: 7,
        mint: key(2),
        source_token: key(3),
        beneficiary: key(4),
        authority: key(5),
        total_amount: 1_000,
        start_ts: 10,
        cliff_ts: 20,
        end_ts: 30,
        revocable: true,
    }
}

/// (address, is_signer, is_writable)
fn flags(ix: &solana_program::instruction::Instruction) -> Vec<(Pubkey, bool, bool)> {
    ix.accounts.iter().map(|a| (a.pubkey, a.is_signer, a.is_writable)).collect()
}

#[test]
fn initialize_lists_the_documented_accounts() {
    let p = params();
    let ix = initialize_schedule(&ID, &p);
    let schedule = schedule_address(&ID, &p.creator, &p.mint, 7).0;
    assert_eq!(ix.program_id, ID);
    assert_eq!(
        flags(&ix),
        vec![
            (p.creator, true, true),
            (schedule, false, true),
            (p.mint, false, false),
            (p.source_token, false, true),
            (vault_address(&schedule, &p.mint), false, true),
            (system_program::ID, false, false),
            (spl_token::ID, false, false),
        ]
    );
    assert_eq!(p.schedule(&ID), schedule);
    match VestingInstruction::unpack(&ix.data).unwrap() {
        VestingInstruction::InitializeSchedule(d) => {
            assert_eq!((d.schedule_id, d.total_amount, d.start_ts, d.cliff_ts, d.end_ts, d.revocable), (7, 1_000, 10, 20, 30, true));
            assert_eq!((d.beneficiary, d.authority), (p.beneficiary, p.authority));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn claim_revoke_and_close_derive_the_vault_from_the_schedule_and_mint() {
    let (schedule, mint) = (key(10), key(2));
    let vault = vault_address(&schedule, &mint);
    let claim = claim(&ID, &key(4), &schedule, &mint, &key(6));
    assert_eq!(
        flags(&claim),
        vec![
            (key(4), true, false),
            (schedule, false, true),
            (vault, false, true),
            (key(6), false, true),
            (mint, false, false),
            (spl_token::ID, false, false)
        ]
    );
    assert_eq!(claim.data, vec![1]);

    let revoke = revoke(&ID, &key(5), &schedule, &mint, &key(6), &key(7));
    assert_eq!(
        flags(&revoke),
        vec![
            (key(5), true, false),
            (schedule, false, true),
            (vault, false, true),
            (key(6), false, true),
            (key(7), false, true),
            (mint, false, false),
            (spl_token::ID, false, false)
        ]
    );
    assert_eq!(revoke.data, vec![2]);

    let close = close(&ID, &key(5), &schedule, &mint, &key(7));
    assert_eq!(
        flags(&close),
        vec![
            (key(5), true, true),
            (schedule, false, true),
            (vault, false, true),
            (key(7), false, true),
            (mint, false, false),
            (spl_token::ID, false, false)
        ]
    );
    assert_eq!(close.data, vec![3]);
}

#[test]
fn beneficiary_change_instructions() {
    let schedule = key(10);
    let propose = propose_beneficiary(&ID, &key(4), &schedule, &key(9));
    assert_eq!(flags(&propose), vec![(key(4), true, false), (schedule, false, true)]);
    assert_eq!(VestingInstruction::unpack(&propose.data).unwrap(), VestingInstruction::ProposeBeneficiary { new_beneficiary: key(9) });
    let accept = accept_beneficiary(&ID, &key(9), &schedule);
    assert_eq!(flags(&accept), vec![(key(9), true, false), (schedule, false, true)]);
    assert_eq!(accept.data, vec![5]);
}

#[test]
fn create_vault_is_the_idempotent_associated_token_account_instruction() {
    let (payer, wallet, mint) = (key(1), key(2), key(3));
    let ix = create_vault(&payer, &wallet, &mint);
    assert_eq!(ix.program_id, ASSOCIATED_TOKEN_PROGRAM_ID);
    assert_eq!(ix.data, vec![1], "CreateIdempotent");
    assert_eq!(
        flags(&ix),
        vec![
            (payer, true, true),
            (vault_address(&wallet, &mint), false, true),
            (wallet, false, false),
            (mint, false, false),
            (system_program::ID, false, false),
            (spl_token::ID, false, false)
        ]
    );
}

#[test]
fn status_follows_the_pure_logic_and_a_revocation() {
    let mut state = ScheduleAccount {
        bump: 255,
        revocable: true,
        revoked: false,
        id: 1,
        creator: key(1),
        authority: key(5),
        beneficiary: key(4),
        pending_beneficiary: Pubkey::default(),
        mint: key(2),
        vault: key(8),
        total_amount: 1_000,
        claimed_amount: 100,
        returned_amount: 0,
        start_ts: 0,
        cliff_ts: 0,
        end_ts: 1_000,
        revoked_at: 0,
    };
    assert_eq!(status(&state, 500).unwrap(), Status { vested: 500, claimed: 100, claimable: 400, unvested: 500 });
    assert_eq!(status(&state, 5_000).unwrap(), Status { vested: 1_000, claimed: 100, claimable: 900, unvested: 0 });
    state.revoked = true;
    state.claimed_amount = 500;
    state.returned_amount = 500;
    assert_eq!(status(&state, 9_999).unwrap(), Status { vested: 500, claimed: 500, claimable: 0, unvested: 0 });
}

#[test]
fn errors_can_be_named() {
    assert!(describe_error(25).starts_with("NotAuthority (25)"));
    assert!(describe_error(9_999).contains("not an error of this program"));
}
