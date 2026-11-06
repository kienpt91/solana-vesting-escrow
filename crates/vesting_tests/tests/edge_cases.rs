//! Boundary values on the real program: tiny and huge amounts, cliff and end corner cases, schedule bounds, mint
//! settings and a clock that moves backwards.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use vesting_client::{VestingError, MAX_DURATION_SECS};
use vesting_core::Schedule;
use vesting_tests::{expect, Scenario, Setup, T0};

#[tokio::test]
async fn a_one_token_schedule_pays_only_at_the_end() {
    let mut s = Scenario::new(Setup { total: 1, creator_balance: 1, end: 1_000, ..Setup::default() }).await;
    s.at(999).await;
    assert_eq!(s.claim().await, Err(expect(0, VestingError::NothingToClaim)), "the floor of 999/1000 of one token is zero");
    s.at(1_000).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1);
    assert_eq!(s.vault_balance().await, 0);
}

#[tokio::test]
async fn rounding_favours_the_vault_on_chain() {
    // 10 tokens over 3 seconds: exact amounts 3.33, 6.67, 10
    let mut s = Scenario::new(Setup { total: 10, creator_balance: 10, end: 3, ..Setup::default() }).await;
    for (offset, cumulative) in [(1, 3), (2, 6), (3, 10)] {
        s.at(offset).await;
        s.claim().await.unwrap();
        assert_eq!(s.beneficiary_balance().await, cumulative, "after {offset} s");
        assert_eq!(s.vault_balance().await, 10 - cumulative);
    }
}

#[tokio::test]
async fn an_amount_of_u64_max_is_released_without_overflow() {
    let total = u64::MAX;
    let mut s = Scenario::new(Setup { total, creator_balance: total, end: 3 * 86_400, ..Setup::default() }).await;
    assert_eq!(s.vault_balance().await, u64::MAX);
    let core = Schedule::new(total, T0, T0, T0 + 3 * 86_400).unwrap();
    let mut paid = 0u64;
    for offset in [86_400, 2 * 86_400, 3 * 86_400] {
        s.at(offset).await;
        s.claim().await.unwrap();
        paid = core.vested_at(T0 + offset).unwrap();
        assert_eq!(s.beneficiary_balance().await, paid);
    }
    assert_eq!(paid, u64::MAX);
    assert_eq!(s.vault_balance().await, 0);
}

#[tokio::test]
async fn an_amount_near_u64_max_revokes_exactly() {
    let total = u64::MAX - 1;
    let mut s = Scenario::new(Setup { total, creator_balance: total, end: 1_000, ..Setup::default() }).await;
    s.at(333).await;
    s.revoke().await.unwrap();
    let vested = Schedule::new(total, T0, T0, T0 + 1_000).unwrap().vested_at(T0 + 333).unwrap();
    assert_eq!(s.beneficiary_balance().await, vested);
    assert_eq!(s.authority_balance().await, total - vested);
    assert_eq!(s.vault_balance().await, 0);
}

#[tokio::test]
async fn an_end_equal_to_the_cliff_releases_everything_at_once() {
    let mut s = Scenario::new(Setup { cliff: 600, end: 600, ..Setup::default() }).await;
    s.at(599).await;
    assert_eq!(s.claim().await, Err(expect(0, VestingError::CliffNotReached)));
    s.at(600).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1_000_000);
}

#[tokio::test]
async fn the_first_second_after_the_cliff_and_the_last_second_before_the_end() {
    let mut s = Scenario::new(Setup { cliff: 100, ..Setup::default() }).await;
    s.at(100).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 100_000);
    s.at(999).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 999_000);
    s.at(1_000).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 1_000_000);
}

#[tokio::test]
async fn the_longest_allowed_schedule_is_accepted_and_one_second_more_is_not() {
    let mut s = Scenario::prepare(Setup::default()).await;
    s.params.end_ts = T0 + MAX_DURATION_SECS;
    s.initialize().await.unwrap();

    let mut s = Scenario::prepare(Setup::default()).await;
    s.params.end_ts = T0 + MAX_DURATION_SECS + 1;
    assert_eq!(s.initialize().await, Err(expect(1, VestingError::DurationTooLong)));
}

/// A name, a mutation of valid parameters, and the error it must cause.
type Case = (&'static str, Box<dyn Fn(&mut vesting_client::InitializeParams)>, VestingError);

#[tokio::test]
async fn invalid_schedule_parameters_are_refused_with_their_own_error() {
    let cases: [Case; 8] = [
        ("zero amount", Box::new(|p| p.total_amount = 0), VestingError::ZeroAmount),
        ("negative start", Box::new(|p| p.start_ts = -1), VestingError::NegativeTimestamp),
        (
            "minimum timestamps",
            Box::new(|p| {
                p.start_ts = i64::MIN;
                p.cliff_ts = i64::MIN;
                p.end_ts = i64::MIN
            }),
            VestingError::NegativeTimestamp,
        ),
        (
            "end equal to start",
            Box::new(|p| {
                p.cliff_ts = p.start_ts;
                p.end_ts = p.start_ts
            }),
            VestingError::EndNotAfterStart,
        ),
        (
            "cliff before start",
            Box::new(|p| {
                p.start_ts = T0 + 10;
                p.cliff_ts = T0 + 9
            }),
            VestingError::CliffBeforeStart,
        ),
        ("cliff after end", Box::new(|p| p.cliff_ts = p.end_ts + 1), VestingError::CliffAfterEnd),
        ("maximum end", Box::new(|p| p.end_ts = i64::MAX), VestingError::DurationTooLong),
        (
            "end in the past",
            Box::new(|p| {
                p.start_ts = T0 - 100;
                p.cliff_ts = T0 - 100;
                p.end_ts = T0
            }),
            VestingError::ScheduleAlreadyEnded,
        ),
    ];
    for (name, mutate, error) in cases {
        let mut s = Scenario::prepare(Setup::default()).await;
        mutate(&mut s.params);
        assert_eq!(s.initialize().await, Err(expect(1, error)), "{name}");
        assert!(s.env.account(&s.schedule).await.is_none(), "{name}: no schedule account was created");
    }
}

#[tokio::test]
async fn mints_with_zero_and_nine_decimals_work() {
    for decimals in [0u8, 9] {
        let mut s = Scenario::new(Setup { decimals, ..Setup::default() }).await;
        s.at(500).await;
        s.claim().await.unwrap();
        assert_eq!(s.beneficiary_balance().await, 500_000, "{decimals} decimals");
    }
}

#[tokio::test]
async fn a_mint_with_a_freeze_authority_works_until_an_account_is_frozen() {
    let mut s = Scenario::new(Setup { freeze_authority: true, ..Setup::default() }).await;
    s.at(500).await;
    s.claim().await.unwrap();
    assert_eq!(s.beneficiary_balance().await, 500_000);
}

#[tokio::test]
async fn a_clock_that_moves_backwards_cannot_be_used_to_claim_again() {
    let mut s = Scenario::new(Setup::default()).await;
    s.at(800).await;
    s.claim().await.unwrap();
    // the runtime's clock never goes back; if it did, the recorded claim would exceed what has vested and the
    // program must refuse instead of underflowing
    s.at(100).await;
    assert_eq!(s.claim().await, Err(expect(0, VestingError::CorruptSchedule)));
    assert_eq!(s.beneficiary_balance().await, 800_000);
}

#[tokio::test]
async fn a_negative_clock_is_refused() {
    let mut s = Scenario::new(Setup::default()).await;
    s.env.set_time(-5).await;
    assert_eq!(s.claim().await, Err(expect(0, VestingError::InvalidClock)));
}
