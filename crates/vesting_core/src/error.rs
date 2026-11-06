use core::fmt;

/// Why a set of schedule parameters is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// The total amount is zero: there would be nothing to vest.
    ZeroAmount,
    /// Start, cliff or end is negative (before 1970).
    NegativeTimestamp,
    /// The end is not strictly after the start.
    EndNotAfterStart,
    /// The cliff is before the start.
    CliffBeforeStart,
    /// The cliff is after the end.
    CliffAfterEnd,
    /// End minus start exceeds [`crate::MAX_DURATION_SECS`].
    DurationTooLong,
    /// The end is not in the future, so nothing would be locked.
    AlreadyEnded,
}

/// Why an accounting question cannot be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountingError {
    /// A checked operation overflowed or underflowed.
    Overflow,
    /// The recorded claimed amount is larger than the total amount.
    ClaimedExceedsTotal,
    /// The recorded claimed amount is larger than what has vested: the state is inconsistent with the clock.
    ClaimedExceedsVested,
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ZeroAmount => "total amount is zero",
            Self::NegativeTimestamp => "a timestamp is negative",
            Self::EndNotAfterStart => "end is not after start",
            Self::CliffBeforeStart => "cliff is before start",
            Self::CliffAfterEnd => "cliff is after end",
            Self::DurationTooLong => "duration exceeds the maximum",
            Self::AlreadyEnded => "end is not in the future",
        })
    }
}

impl fmt::Display for AccountingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overflow => "arithmetic overflow",
            Self::ClaimedExceedsTotal => "claimed amount exceeds the total",
            Self::ClaimedExceedsVested => "claimed amount exceeds the vested amount",
        })
    }
}

impl std::error::Error for ScheduleError {}
impl std::error::Error for AccountingError {}
