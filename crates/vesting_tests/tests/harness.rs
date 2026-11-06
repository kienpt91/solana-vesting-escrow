//! Tests of the harness itself: the evidence the attack suite prints has to say what really happened.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use solana_sdk::signer::Signer;
use vesting_tests::{attack, Scenario, Setup};

/// A hostile transfer sent straight to the SPL Token program fails with the token program's `OwnerMismatch`
/// (code 4). The vesting program's error number 4 is `CliffAfterEnd`, which is not what happened.
#[tokio::test]
async fn an_error_of_the_token_program_is_not_labelled_as_a_vesting_error() {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(500).await;
    let stranger = s.stranger.insecure_clone();
    let ix = spl_token::instruction::transfer(&spl_token::ID, &s.vault, &s.stranger_token, &stranger.pubkey(), &[], 1).unwrap();
    let failure = s.env.send(&[ix], &[&stranger]).await.expect_err("the transfer must fail");
    let text = failure.describe();
    assert!(text.contains("SPL Token"), "the program of origin is named: {text}");
    assert!(text.contains("OwnerMismatch"), "the token error is named: {text}");
    assert!(!text.contains("CliffAfterEnd"), "a token error must not wear a vesting name: {text}");
}

#[tokio::test]
async fn an_error_of_the_vesting_program_keeps_its_name_and_its_origin() {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(500).await;
    let stranger = s.stranger.insecure_clone();
    let ix = vesting_client::claim(&s.program_id(), &stranger.pubkey(), &s.schedule, &s.mint, &s.stranger_token);
    let failure = s.env.send(&[ix], &[&stranger]).await.expect_err("the claim must fail");
    let text = failure.describe();
    assert!(text.contains("vesting"), "{text}");
    assert!(text.contains("NotBeneficiary"), "{text}");
}

#[tokio::test]
async fn a_system_program_error_names_the_system_program() {
    let mut s = Scenario::new(Setup::default()).await;
    let stranger = s.stranger.insecure_clone();
    // more lamports than the account holds: the system program's `ResultWithNegativeLamports` (custom code 1)
    let ix = solana_sdk::system_instruction::transfer(&stranger.pubkey(), &s.creator.pubkey(), u64::MAX);
    let failure = s.env.send(&[ix], &[&stranger]).await.expect_err("the transfer must fail");
    let text = failure.describe();
    assert!(text.contains("system program"), "{text}");
    assert!(!text.contains("ZeroAmount") && !text.contains("NegativeTimestamp"), "a system error must not wear a vesting name: {text}");
}

#[test]
fn a_row_of_the_attack_table_starts_on_its_own_line_and_is_numbered() {
    // libtest prints "test NAME ... " without a newline before a test runs; a row that is not preceded by a newline
    // would be glued to it and lost by any filter that anchors on the start of the line
    let first = attack::row("one", "expected one", "observed one", true);
    let second = attack::row("two", "expected two", "observed two", false);
    assert!(first.starts_with("\nATTACK|"), "{first:?}");
    assert!(second.starts_with("\nATTACK|"), "{second:?}");
    let number = |row: &str| row.trim_start().split('|').nth(1).unwrap().parse::<u64>().unwrap();
    assert!(number(&second) > number(&first), "rows are numbered in the order they are printed");
    assert!(first.trim_end().ends_with("|PASS"));
    assert!(second.trim_end().ends_with("|FAIL"));
    assert_eq!(first.trim().split('|').count(), 6, "six fields: tag, number, name, expected, observed, verdict");
}
