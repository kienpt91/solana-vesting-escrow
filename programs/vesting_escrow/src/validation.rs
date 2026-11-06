//! The account-validation layer: small checks that every handler composes. Each one names the exact error, so a
//! hostile account list fails with the check that caught it. The runtime would stop some of these later with a
//! generic error; checking first keeps the failure precise and keeps the checks from depending on the runtime.

use crate::error::VestingError;
use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

/// Splits the account list into exactly `N` accounts. Fewer is `NotEnoughAccountKeys`, more is
/// `UnexpectedAccounts`: extra accounts are refused rather than ignored.
pub fn exact<'a, 'info, const N: usize>(accounts: &'a [AccountInfo<'info>]) -> Result<&'a [AccountInfo<'info>; N], ProgramError> {
    if accounts.len() < N {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    <&[AccountInfo<'info>; N]>::try_from(accounts).map_err(|_| VestingError::UnexpectedAccounts.into())
}

/// The account signed the transaction.
pub fn signer(account: &AccountInfo) -> Result<(), ProgramError> {
    if account.is_signer {
        Ok(())
    } else {
        Err(ProgramError::MissingRequiredSignature)
    }
}

/// The account is writable in this transaction.
pub fn writable(account: &AccountInfo) -> Result<(), ProgramError> {
    if account.is_writable {
        Ok(())
    } else {
        Err(VestingError::AccountNotWritable.into())
    }
}

/// The account has exactly this address.
pub fn key_is(account: &AccountInfo, expected: &Pubkey, error: VestingError) -> Result<(), ProgramError> {
    if account.key == expected {
        Ok(())
    } else {
        Err(error.into())
    }
}

/// The account is owned by `owner`.
pub fn owned_by(account: &AccountInfo, owner: &Pubkey, error: VestingError) -> Result<(), ProgramError> {
    if account.owner == owner {
        Ok(())
    } else {
        Err(error.into())
    }
}

/// No two of the accounts are the same account.
pub fn distinct(accounts: &[&AccountInfo]) -> Result<(), ProgramError> {
    for (i, a) in accounts.iter().enumerate() {
        for b in accounts.iter().skip(i.saturating_add(1)) {
            if a.key == b.key {
                return Err(VestingError::DuplicateAccount.into());
            }
        }
    }
    Ok(())
}
