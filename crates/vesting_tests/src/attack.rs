//! The attack table. Every attack test records the outcome it expected and the outcome it observed, and prints one
//! machine-readable row per hostile transaction: `ATTACK|seq|name|expected|observed|verdict`. `scripts/demo.sh` collects the
//! rows into a table and `scripts/attack-table.sh` refuses the table when a row is missing.
//!
//! A row starts with a newline. libtest prints `test NAME ... ` without one before a test runs, so a row that did not start
//! on its own line would be glued to it and lost by a filter that matches the start of a line.

use crate::{expect, Failure};
use std::sync::atomic::{AtomicU64, Ordering};
use vesting_client::VestingError;

/// Rows printed so far by this process; the number of a row is its place in the output.
static ROWS: AtomicU64 = AtomicU64::new(0);

/// One row of the table, numbered with the next number. `passed` is the verdict.
pub fn row(name: &str, expected: &str, observed: &str, passed: bool) -> String {
    let number = ROWS.fetch_add(1, Ordering::SeqCst).saturating_add(1);
    let verdict = if passed { "PASS" } else { "FAIL" };
    format!("\nATTACK|{number}|{name}|{expected}|{observed}|{verdict}")
}

fn report(name: &str, result: Result<(), Failure>, want: Failure, expected_text: &str) {
    match result {
        Ok(()) => {
            println!("{}", row(name, expected_text, "transaction succeeded", false));
            panic!("attack `{name}` was not rejected: expected {expected_text}");
        }
        Err(got) => {
            let observed = got.describe();
            println!("{}", row(name, expected_text, &observed, got == want));
            assert_eq!(got, want, "attack `{name}`: expected {expected_text}, observed {observed}");
        }
    }
}

/// Asserts that the transaction failed with exactly `expected` from the vesting program at instruction `index`, prints
/// the row and returns normally; panics with both outcomes otherwise.
pub fn assert_rejected(name: &str, result: Result<(), Failure>, index: u8, expected: VestingError) {
    let want = expect(index, expected);
    let expected_text = want_text(&want);
    report(name, result, want, &expected_text);
}

/// Like [`assert_rejected`] for a built-in error (missing signature, bad instruction data and so on) that the vesting
/// program returns.
pub fn assert_rejected_with(name: &str, result: Result<(), Failure>, index: u8, expected: solana_sdk::instruction::InstructionError) {
    assert_rejected_by(name, result, index, &vesting_escrow::ID, expected);
}

/// Like [`assert_rejected_with`] for an instruction addressed to another program (a hostile call straight to the token
/// program or to the system program): the error and the program that must return it.
pub fn assert_rejected_by(
    name: &str,
    result: Result<(), Failure>,
    index: u8,
    program: &solana_sdk::pubkey::Pubkey,
    expected: solana_sdk::instruction::InstructionError,
) {
    let want = Failure::Instruction { index, program: *program, error: expected };
    let expected_text = want_text(&want);
    report(name, result, want, &expected_text);
}

fn want_text(want: &Failure) -> String {
    want.describe()
}
