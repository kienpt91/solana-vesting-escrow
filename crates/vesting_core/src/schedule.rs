use crate::error::{AccountingError, ScheduleError};

/// Longest accepted schedule: ten years of 365 days, in seconds.
pub const MAX_DURATION_SECS: i64 = 10 * 365 * 24 * 60 * 60;

/// A validated vesting schedule: `total` tokens that vest linearly from `start` to `end`, with nothing
/// released before `cliff`.
///
/// Invariants, enforced by [`Schedule::new`]: `total > 0`, `0 <= start <= cliff <= end`, `start < end` and
/// `end - start <= MAX_DURATION_SECS`. Because the fields are private, every value of this type satisfies them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    total: u64,
    start: i64,
    cliff: i64,
    end: i64,
}

impl Schedule {
    /// Validates the parameters. Timestamps are Unix seconds. A cliff equal to the start means no cliff; a cliff
    /// equal to the end releases everything at once at that time.
    pub fn new(total: u64, start: i64, cliff: i64, end: i64) -> Result<Self, ScheduleError> {
        if total == 0 {
            return Err(ScheduleError::ZeroAmount);
        }
        if start < 0 || cliff < 0 || end < 0 {
            return Err(ScheduleError::NegativeTimestamp);
        }
        if end <= start {
            return Err(ScheduleError::EndNotAfterStart);
        }
        if cliff < start {
            return Err(ScheduleError::CliffBeforeStart);
        }
        if cliff > end {
            return Err(ScheduleError::CliffAfterEnd);
        }
        match end.checked_sub(start) {
            Some(duration) if duration <= MAX_DURATION_SECS => {}
            _ => return Err(ScheduleError::DurationTooLong),
        }
        Ok(Self { total, start, cliff, end })
    }

    /// Like [`Schedule::new`], and additionally refuses a schedule whose end is not after `now`: such a schedule
    /// would lock nothing. A start in the past is accepted (the schedule is back-dated).
    pub fn new_at(total: u64, start: i64, cliff: i64, end: i64, now: i64) -> Result<Self, ScheduleError> {
        let schedule = Self::new(total, start, cliff, end)?;
        if schedule.end <= now {
            return Err(ScheduleError::AlreadyEnded);
        }
        Ok(schedule)
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn start(&self) -> i64 {
        self.start
    }

    pub fn cliff(&self) -> i64 {
        self.cliff
    }

    pub fn end(&self) -> i64 {
        self.end
    }

    /// The amount vested at `now` (Unix seconds): zero before the cliff, `total` from the end on, and in
    /// between `floor(total * (now - start) / (end - start))`. The floor is what makes rounding favour the vault.
    /// The function is monotonic in `now` and never exceeds `total`.
    pub fn vested_at(&self, now: i64) -> Result<u64, AccountingError> {
        if now < self.cliff {
            return Ok(0);
        }
        if now >= self.end {
            return Ok(self.total);
        }
        // here cliff <= now < end, hence start <= now < end and both differences are positive
        let elapsed = span(self.start, now)?;
        let duration = span(self.start, self.end)?;
        let product = u128::from(self.total).checked_mul(u128::from(elapsed)).ok_or(AccountingError::Overflow)?;
        let vested = product.checked_div(u128::from(duration)).ok_or(AccountingError::Overflow)?;
        u64::try_from(vested).map_err(|_| AccountingError::Overflow)
    }

    /// True once everything has vested.
    pub fn is_fully_vested(&self, now: i64) -> bool {
        now >= self.end
    }

    /// What can be claimed now: vested minus already claimed. Fails if the recorded claimed amount exceeds the
    /// total or the vested amount, which a consistent state never does.
    pub fn claimable(&self, now: i64, claimed: u64) -> Result<u64, AccountingError> {
        if claimed > self.total {
            return Err(AccountingError::ClaimedExceedsTotal);
        }
        let vested = self.vested_at(now)?;
        vested.checked_sub(claimed).ok_or(AccountingError::ClaimedExceedsVested)
    }

    /// The claimed amount after paying out `amount` more. It can never exceed the total.
    pub fn after_claim(&self, claimed: u64, amount: u64) -> Result<u64, AccountingError> {
        let next = claimed.checked_add(amount).ok_or(AccountingError::Overflow)?;
        if next > self.total {
            return Err(AccountingError::ClaimedExceedsTotal);
        }
        Ok(next)
    }

    /// True when every token has left the vault: claimed by the beneficiary or returned to the authority.
    pub fn is_settled(&self, claimed: u64, returned: u64) -> Result<bool, AccountingError> {
        let paid = claimed.checked_add(returned).ok_or(AccountingError::Overflow)?;
        if paid > self.total {
            return Err(AccountingError::ClaimedExceedsTotal);
        }
        Ok(paid == self.total)
    }
}

/// `to - from` as an unsigned number of seconds; `to >= from` is expected, anything else is an error.
fn span(from: i64, to: i64) -> Result<u64, AccountingError> {
    let diff = to.checked_sub(from).ok_or(AccountingError::Overflow)?;
    u64::try_from(diff).map_err(|_| AccountingError::Overflow)
}
