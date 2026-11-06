//! Program errors. Every rejected condition has its own code so that a test (or a client) can tell exactly which
//! check fired. Conditions that the runtime already names (missing signature, bad instruction data, an account
//! that is already initialized, too few accounts) use the matching built-in `ProgramError`.

use core::fmt;
use solana_program::program_error::ProgramError;
use vesting_core::{AccountingError, ScheduleError};

macro_rules! errors {
    ($($name:ident = $code:expr => $text:expr,)+) => {
        /// A rejected condition. The discriminant is the code of `ProgramError::Custom`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum VestingError {
            $($name = $code,)+
        }

        impl VestingError {
            /// Every error, in code order.
            pub const ALL: &'static [VestingError] = &[$(VestingError::$name,)+];

            /// The number carried by `ProgramError::Custom`.
            pub const fn code(self) -> u32 {
                self as u32
            }

            /// The error with this code, if there is one.
            pub fn from_code(code: u32) -> Option<VestingError> {
                match code {
                    $($code => Some(VestingError::$name),)+
                    _ => None,
                }
            }

            /// One line of human-readable text.
            pub const fn description(self) -> &'static str {
                match self {
                    $(VestingError::$name => $text,)+
                }
            }
        }
    };
}

errors! {
    ZeroAmount = 0 => "total amount is zero",
    NegativeTimestamp = 1 => "start, cliff or end is negative",
    EndNotAfterStart = 2 => "end is not after start",
    CliffBeforeStart = 3 => "cliff is before start",
    CliffAfterEnd = 4 => "cliff is after end",
    DurationTooLong = 5 => "schedule is longer than the maximum duration",
    ScheduleAlreadyEnded = 6 => "end is not in the future",
    InvalidParty = 7 => "a beneficiary or authority key is the default (all zero) key or unchanged",
    WrongSystemProgram = 8 => "the system program account is not the system program",
    WrongTokenProgram = 9 => "the token program account is not the SPL Token program",
    UnsupportedTokenProgram = 10 => "Token-2022 is not supported",
    WrongAccountOwner = 11 => "an account is owned by the wrong program",
    WrongMint = 12 => "the mint account is not the mint of the schedule",
    WrongVault = 13 => "the vault is not the vault of the schedule",
    WrongScheduleAddress = 14 => "the schedule account is not the canonical program derived address",
    WrongTokenAccountMint = 15 => "a token account holds a different mint",
    WrongTokenAccountOwner = 16 => "a token account has the wrong owner",
    TokenAccountFrozen = 17 => "a token account is frozen",
    UnsafeVaultAuthority = 18 => "the vault has a delegate or a close authority",
    InsufficientFunds = 19 => "the source token account holds less than the total amount",
    VaultFundingMismatch = 20 => "the vault balance did not grow by exactly the total amount",
    DuplicateAccount = 21 => "the same account was passed for two roles that must differ",
    AccountNotWritable = 22 => "an account that is modified is not writable",
    UnexpectedAccounts = 23 => "more accounts than the instruction takes",
    NotBeneficiary = 24 => "the signer is not the beneficiary of the schedule",
    NotAuthority = 25 => "the signer is not the authority of the schedule",
    NotPendingBeneficiary = 26 => "the signer is not the proposed beneficiary",
    NoPendingBeneficiary = 27 => "no beneficiary change is pending",
    NotRevocable = 28 => "the schedule is not revocable",
    ScheduleRevoked = 29 => "the schedule is already revoked",
    FullyVested = 30 => "everything has vested, there is nothing to revoke",
    CliffNotReached = 31 => "the cliff has not been reached",
    NothingToClaim = 32 => "nothing is claimable right now",
    NotSettled = 33 => "tokens are still owed: the schedule is not settled",
    InvalidClock = 34 => "the clock reports a negative time",
    CorruptSchedule = 35 => "the stored schedule is inconsistent",
    MathOverflow = 36 => "arithmetic overflow",
}

impl fmt::Display for VestingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

impl std::error::Error for VestingError {}

impl From<VestingError> for ProgramError {
    fn from(e: VestingError) -> Self {
        ProgramError::Custom(e.code())
    }
}

impl From<ScheduleError> for VestingError {
    fn from(e: ScheduleError) -> Self {
        match e {
            ScheduleError::ZeroAmount => VestingError::ZeroAmount,
            ScheduleError::NegativeTimestamp => VestingError::NegativeTimestamp,
            ScheduleError::EndNotAfterStart => VestingError::EndNotAfterStart,
            ScheduleError::CliffBeforeStart => VestingError::CliffBeforeStart,
            ScheduleError::CliffAfterEnd => VestingError::CliffAfterEnd,
            ScheduleError::DurationTooLong => VestingError::DurationTooLong,
            ScheduleError::AlreadyEnded => VestingError::ScheduleAlreadyEnded,
        }
    }
}

impl From<AccountingError> for VestingError {
    fn from(e: AccountingError) -> Self {
        match e {
            AccountingError::Overflow => VestingError::MathOverflow,
            AccountingError::ClaimedExceedsTotal | AccountingError::ClaimedExceedsVested => VestingError::CorruptSchedule,
        }
    }
}

/// Converts a pure-logic error into the `ProgramError` the runtime reports.
pub fn lift<E: Into<VestingError>>(error: E) -> ProgramError {
    error.into().into()
}
