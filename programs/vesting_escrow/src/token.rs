//! Everything that touches the SPL Token program: reading and validating mint and token accounts, and the two
//! CPIs the program makes (`TransferChecked` and `CloseAccount`).
//!
//! Only the classic SPL Token program is supported. Token-2022 mints and token accounts are refused with a
//! dedicated error instead of being handled partially: extensions such as transfer fees, transfer hooks, a
//! permanent delegate or confidential transfers all change what "the vault holds `total` tokens" means.

use crate::error::VestingError;
use solana_program::{
    account_info::AccountInfo,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

/// The Token-2022 program, named only to refuse it precisely.
pub const TOKEN_2022_PROGRAM_ID: Pubkey = solana_program::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

/// The account is the SPL Token program; the Token-2022 program gets its own error.
pub fn check_program(account: &AccountInfo) -> Result<(), ProgramError> {
    if *account.key == spl_token::ID {
        Ok(())
    } else if *account.key == TOKEN_2022_PROGRAM_ID {
        Err(VestingError::UnsupportedTokenProgram.into())
    } else {
        Err(VestingError::WrongTokenProgram.into())
    }
}

/// An account that must be owned by the SPL Token program: Token-2022 ownership is `UnsupportedTokenProgram`,
/// any other owner is `WrongAccountOwner`.
fn check_token_owner(account: &AccountInfo) -> Result<(), ProgramError> {
    if *account.owner == spl_token::ID {
        Ok(())
    } else if *account.owner == TOKEN_2022_PROGRAM_ID {
        Err(VestingError::UnsupportedTokenProgram.into())
    } else {
        Err(VestingError::WrongAccountOwner.into())
    }
}

/// Reads an initialized mint owned by the SPL Token program.
pub fn load_mint(account: &AccountInfo) -> Result<Mint, ProgramError> {
    check_token_owner(account)?;
    Mint::unpack(&account.try_borrow_data()?)
}

/// Reads an initialized token account owned by the SPL Token program.
pub fn load_account(account: &AccountInfo) -> Result<TokenAccount, ProgramError> {
    check_token_owner(account)?;
    TokenAccount::unpack(&account.try_borrow_data()?)
}

/// A token account that sends or receives tokens: right mint, right owner, not frozen. A delegate or a close
/// authority on a destination is allowed: it cannot take anything out of the vault, only out of the destination.
pub fn check_account(account: &TokenAccount, mint: &Pubkey, owner: &Pubkey) -> Result<(), ProgramError> {
    if account.mint != *mint {
        return Err(VestingError::WrongTokenAccountMint.into());
    }
    if account.owner != *owner {
        return Err(VestingError::WrongTokenAccountOwner.into());
    }
    if account.state == AccountState::Frozen {
        return Err(VestingError::TokenAccountFrozen.into());
    }
    Ok(())
}

/// The vault: right mint, owned by the schedule address, not frozen, and with neither a delegate nor a close
/// authority. Only the owner can set those and this program never does, so a vault that has them was not
/// created by the intended path.
pub fn check_vault(account: &TokenAccount, mint: &Pubkey, schedule: &Pubkey) -> Result<(), ProgramError> {
    check_account(account, mint, schedule)?;
    if account.delegate != COption::None || account.close_authority != COption::None {
        return Err(VestingError::UnsafeVaultAuthority.into());
    }
    Ok(())
}

/// Moves `amount` tokens with `TransferChecked`, signed by the transaction (the creator).
pub fn transfer<'info>(
    token_program: &AccountInfo<'info>,
    source: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    decimals: u8,
) -> Result<(), ProgramError> {
    let ix = spl_token::instruction::transfer_checked(
        token_program.key,
        source.key,
        mint.key,
        destination.key,
        authority.key,
        &[],
        amount,
        decimals,
    )?;
    invoke(&ix, &[source.clone(), mint.clone(), destination.clone(), authority.clone(), token_program.clone()])
}

/// Moves `amount` tokens out of the vault with `TransferChecked`; the schedule address signs with `seeds`.
#[allow(clippy::too_many_arguments)]
pub fn transfer_from_vault<'info>(
    token_program: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    schedule: &AccountInfo<'info>,
    amount: u64,
    decimals: u8,
    seeds: &[&[u8]],
) -> Result<(), ProgramError> {
    let ix = spl_token::instruction::transfer_checked(
        token_program.key,
        vault.key,
        mint.key,
        destination.key,
        schedule.key,
        &[],
        amount,
        decimals,
    )?;
    invoke_signed(&ix, &[vault.clone(), mint.clone(), destination.clone(), schedule.clone(), token_program.clone()], &[seeds])
}

/// Closes the (empty) vault and sends its rent to `destination`; the schedule address signs with `seeds`.
pub fn close_vault<'info>(
    token_program: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    schedule: &AccountInfo<'info>,
    seeds: &[&[u8]],
) -> Result<(), ProgramError> {
    let ix = spl_token::instruction::close_account(token_program.key, vault.key, destination.key, schedule.key, &[])?;
    invoke_signed(&ix, &[vault.clone(), destination.clone(), schedule.clone(), token_program.clone()], &[seeds])
}
