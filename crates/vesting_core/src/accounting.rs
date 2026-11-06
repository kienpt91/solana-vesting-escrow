use crate::error::AccountingError;
use crate::schedule::Schedule;

/// How a revocation at one point in time splits the escrowed amount. `claimed + to_beneficiary + to_authority`
/// always equals the schedule total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revocation {
    /// Everything vested at the revocation time, claimed or not.
    pub vested: u64,
    /// Vested and not yet claimed: paid to the beneficiary.
    pub to_beneficiary: u64,
    /// Not vested: returned to the authority.
    pub to_authority: u64,
}

impl Schedule {
    /// Splits the vault at `now` given the amount `claimed` so far. The beneficiary receives the vested part
    /// that was not claimed yet, the authority gets the unvested remainder.
    pub fn revocation(&self, now: i64, claimed: u64) -> Result<Revocation, AccountingError> {
        let vested = self.vested_at(now)?;
        let to_beneficiary = self.claimable(now, claimed)?;
        let to_authority = self.total().checked_sub(vested).ok_or(AccountingError::Overflow)?;
        Ok(Revocation { vested, to_beneficiary, to_authority })
    }
}
