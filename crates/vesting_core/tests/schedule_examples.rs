//! Worked examples and validation rules of the pure schedule logic.
#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use vesting_core::{AccountingError, Schedule, ScheduleError, MAX_DURATION_SECS};

fn s(total: u64, start: i64, cliff: i64, end: i64) -> Schedule {
    Schedule::new(total, start, cliff, end).expect("valid schedule")
}

#[test]
fn zero_amount_is_refused() {
    assert_eq!(Schedule::new(0, 0, 0, 10), Err(ScheduleError::ZeroAmount));
}

#[test]
fn negative_timestamps_are_refused() {
    assert_eq!(Schedule::new(1, -1, 0, 10), Err(ScheduleError::NegativeTimestamp));
    assert_eq!(Schedule::new(1, 0, -1, 10), Err(ScheduleError::NegativeTimestamp));
    assert_eq!(Schedule::new(1, 0, 0, -10), Err(ScheduleError::NegativeTimestamp));
    assert_eq!(Schedule::new(1, i64::MIN, i64::MIN, i64::MIN), Err(ScheduleError::NegativeTimestamp));
}

#[test]
fn end_must_be_after_start() {
    assert_eq!(Schedule::new(1, 10, 10, 10), Err(ScheduleError::EndNotAfterStart));
    assert_eq!(Schedule::new(1, 10, 10, 9), Err(ScheduleError::EndNotAfterStart));
}

#[test]
fn cliff_must_lie_between_start_and_end() {
    assert_eq!(Schedule::new(1, 10, 9, 20), Err(ScheduleError::CliffBeforeStart));
    assert_eq!(Schedule::new(1, 10, 21, 20), Err(ScheduleError::CliffAfterEnd));
    assert!(Schedule::new(1, 10, 10, 20).is_ok(), "cliff equal to start is no cliff");
    assert!(Schedule::new(1, 10, 20, 20).is_ok(), "cliff equal to end is allowed");
}

#[test]
fn duration_bound_is_inclusive() {
    assert!(Schedule::new(1, 0, 0, MAX_DURATION_SECS).is_ok());
    assert_eq!(Schedule::new(1, 0, 0, MAX_DURATION_SECS + 1), Err(ScheduleError::DurationTooLong));
    assert_eq!(Schedule::new(1, 5, 5, i64::MAX), Err(ScheduleError::DurationTooLong));
    assert_eq!(Schedule::new(1, 0, 0, i64::MAX), Err(ScheduleError::DurationTooLong));
}

#[test]
fn creation_refuses_an_end_that_is_not_in_the_future() {
    assert_eq!(Schedule::new_at(1, 0, 0, 100, 100), Err(ScheduleError::AlreadyEnded));
    assert_eq!(Schedule::new_at(1, 0, 0, 100, 101), Err(ScheduleError::AlreadyEnded));
    assert!(Schedule::new_at(1, 0, 0, 100, 99).is_ok());
    // a start in the past is fine
    assert!(Schedule::new_at(1, 10, 10, 100, 50).is_ok());
}

#[test]
fn vested_is_zero_before_the_cliff_and_linear_after_it() {
    // 1000 tokens over 100 s, cliff at 25 s
    let sch = s(1000, 1000, 1025, 1100);
    assert_eq!(sch.vested_at(0), Ok(0));
    assert_eq!(sch.vested_at(1000), Ok(0));
    assert_eq!(sch.vested_at(1024), Ok(0));
    // at the cliff the linear amount accumulated since the start is released
    assert_eq!(sch.vested_at(1025), Ok(250));
    assert_eq!(sch.vested_at(1050), Ok(500));
    assert_eq!(sch.vested_at(1099), Ok(990));
    assert_eq!(sch.vested_at(1100), Ok(1000));
    assert_eq!(sch.vested_at(i64::MAX), Ok(1000));
}

#[test]
fn end_equal_to_cliff_releases_everything_at_the_cliff() {
    let sch = s(777, 0, 50, 50);
    assert_eq!(sch.vested_at(49), Ok(0));
    assert_eq!(sch.vested_at(50), Ok(777));
}

#[test]
fn no_cliff_vests_from_the_first_second() {
    let sch = s(100, 10, 10, 20);
    assert_eq!(sch.vested_at(10), Ok(0));
    assert_eq!(sch.vested_at(11), Ok(10));
    assert_eq!(sch.vested_at(19), Ok(90));
}

#[test]
fn rounding_goes_down() {
    // 10 tokens over 3 s: exact amounts are 3.33 and 6.67
    let sch = s(10, 0, 0, 3);
    assert_eq!(sch.vested_at(1), Ok(3));
    assert_eq!(sch.vested_at(2), Ok(6));
    assert_eq!(sch.vested_at(3), Ok(10));
}

#[test]
fn tiny_total_vests_only_at_the_end() {
    let sch = s(1, 0, 0, 1000);
    assert_eq!(sch.vested_at(999), Ok(0));
    assert_eq!(sch.vested_at(1000), Ok(1));
}

#[test]
fn huge_total_does_not_overflow() {
    let sch = s(u64::MAX, 0, 0, MAX_DURATION_SECS);
    assert_eq!(sch.vested_at(0), Ok(0));
    assert_eq!(sch.vested_at(MAX_DURATION_SECS), Ok(u64::MAX));
    let half = sch.vested_at(MAX_DURATION_SECS / 2).expect("no overflow");
    assert_eq!(half, u64::MAX / 2, "{half}");
    assert!(sch.vested_at(MAX_DURATION_SECS - 1).expect("no overflow") < u64::MAX);
}

#[test]
fn claimable_is_vested_minus_claimed() {
    let sch = s(1000, 0, 0, 100);
    assert_eq!(sch.claimable(50, 0), Ok(500));
    assert_eq!(sch.claimable(50, 500), Ok(0));
    assert_eq!(sch.claimable(60, 500), Ok(100));
    assert_eq!(sch.claimable(100, 900), Ok(100));
}

#[test]
fn claimable_refuses_an_inconsistent_claimed_amount() {
    let sch = s(1000, 0, 0, 100);
    assert_eq!(sch.claimable(10, 101), Err(AccountingError::ClaimedExceedsVested));
    assert_eq!(sch.claimable(100, 1001), Err(AccountingError::ClaimedExceedsTotal));
    assert_eq!(sch.claimable(100, u64::MAX), Err(AccountingError::ClaimedExceedsTotal));
}

#[test]
fn after_claim_never_passes_the_total() {
    let sch = s(1000, 0, 0, 100);
    assert_eq!(sch.after_claim(400, 600), Ok(1000));
    assert_eq!(sch.after_claim(400, 601), Err(AccountingError::ClaimedExceedsTotal));
    assert_eq!(sch.after_claim(u64::MAX, 1), Err(AccountingError::Overflow));
}

#[test]
fn settled_means_everything_left_the_vault() {
    let sch = s(1000, 0, 0, 100);
    assert_eq!(sch.is_settled(0, 0), Ok(false));
    assert_eq!(sch.is_settled(999, 0), Ok(false));
    assert_eq!(sch.is_settled(1000, 0), Ok(true));
    assert_eq!(sch.is_settled(300, 700), Ok(true));
    assert_eq!(sch.is_settled(300, 701), Err(AccountingError::ClaimedExceedsTotal));
    assert_eq!(sch.is_settled(u64::MAX, 1), Err(AccountingError::Overflow));
}

#[test]
fn revocation_pays_the_vested_part_and_returns_the_rest() {
    let sch = s(1000, 0, 0, 100);
    let r = sch.revocation(40, 100).expect("split");
    assert_eq!((r.vested, r.to_beneficiary, r.to_authority), (400, 300, 600));
    assert_eq!(100 + r.to_beneficiary + r.to_authority, 1000);
}

#[test]
fn revocation_before_the_cliff_returns_everything() {
    let sch = s(1000, 0, 50, 100);
    let r = sch.revocation(10, 0).expect("split");
    assert_eq!((r.vested, r.to_beneficiary, r.to_authority), (0, 0, 1000));
}

#[test]
fn revocation_after_the_end_has_nothing_to_return() {
    let sch = s(1000, 0, 0, 100);
    let r = sch.revocation(100, 250).expect("split");
    assert_eq!((r.vested, r.to_beneficiary, r.to_authority), (1000, 750, 0));
    assert!(sch.is_fully_vested(100));
    assert!(!sch.is_fully_vested(99));
}

#[test]
fn every_error_has_its_own_message() {
    let schedule_errors = [
        ScheduleError::ZeroAmount,
        ScheduleError::NegativeTimestamp,
        ScheduleError::EndNotAfterStart,
        ScheduleError::CliffBeforeStart,
        ScheduleError::CliffAfterEnd,
        ScheduleError::DurationTooLong,
        ScheduleError::AlreadyEnded,
    ];
    let accounting_errors = [AccountingError::Overflow, AccountingError::ClaimedExceedsTotal, AccountingError::ClaimedExceedsVested];
    let mut messages: Vec<String> = schedule_errors.iter().map(ToString::to_string).collect();
    messages.extend(accounting_errors.iter().map(ToString::to_string));
    assert!(messages.iter().all(|m| !m.is_empty()));
    let mut unique = messages.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), messages.len(), "messages must differ: {messages:?}");
    // both are std errors, so callers can box them
    let _: Box<dyn std::error::Error> = Box::new(ScheduleError::ZeroAmount);
    let _: Box<dyn std::error::Error> = Box::new(AccountingError::Overflow);
}
