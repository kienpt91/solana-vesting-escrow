//! The attack table. Every attack test records the outcome it expected and the outcome it observed, and prints one
//! machine-readable line (`ATTACK|name|expected|observed|verdict`) that `scripts/demo.sh` collects into a table.

use crate::{expect, Failure};
use vesting_client::VestingError;

/// Asserts that the transaction failed with exactly `expected` at instruction `index`, prints the table line and
/// returns normally; panics with both outcomes otherwise.
pub fn assert_rejected(name: &str, result: Result<(), Failure>, index: u8, expected: VestingError) {
    let want = expect(index, expected);
    let expected_text = format!("instruction {index}: {expected:?} ({})", expected.code());
    match result {
        Ok(()) => {
            println!("ATTACK|{name}|{expected_text}|transaction succeeded|FAIL");
            panic!("attack `{name}` was not rejected: expected {expected_text}");
        }
        Err(got) => {
            let observed = got.describe();
            let verdict = if got == want { "PASS" } else { "FAIL" };
            println!("ATTACK|{name}|{expected_text}|{observed}|{verdict}");
            assert_eq!(got, want, "attack `{name}`: expected {expected_text}, observed {observed}");
        }
    }
}

/// Like [`assert_rejected`] for a built-in program error (missing signature, bad instruction data and so on).
pub fn assert_rejected_with(name: &str, result: Result<(), Failure>, index: u8, expected: solana_sdk::instruction::InstructionError) {
    let want = Failure::Instruction { index, error: expected.clone() };
    let expected_text = format!("instruction {index}: {expected:?}");
    match result {
        Ok(()) => {
            println!("ATTACK|{name}|{expected_text}|transaction succeeded|FAIL");
            panic!("attack `{name}` was not rejected: expected {expected_text}");
        }
        Err(got) => {
            let observed = got.describe();
            let verdict = if got == want { "PASS" } else { "FAIL" };
            println!("ATTACK|{name}|{expected_text}|{observed}|{verdict}");
            assert_eq!(got, want, "attack `{name}`: expected {expected_text}, observed {observed}");
        }
    }
}
