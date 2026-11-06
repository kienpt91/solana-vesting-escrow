//! Property tests of the schedule math. No external crate: a seeded SplitMix64 generator drives every case, so a
//! failure is reproducible from the printed seed and iteration.
#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use vesting_core::{Schedule, MAX_DURATION_SECS};

/// SplitMix64: tiny, well mixed, and fully determined by its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `lo..=hi`.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(lo <= hi);
        let span = hi - lo;
        if span == u64::MAX {
            return self.next();
        }
        lo + self.next() % (span + 1)
    }

    /// A total that is often an edge value: tiny, huge, near u64::MAX, or uniform.
    fn total(&mut self) -> u64 {
        match self.next() % 6 {
            0 => self.range(1, 10),
            1 => u64::MAX - self.range(0, 10),
            2 => self.range(1, 1_000_000),
            3 => self.range(u64::MAX / 2, u64::MAX),
            4 => 1u64 << self.range(0, 63),
            _ => self.range(1, u64::MAX),
        }
    }
}

const SEED: u64 = 0x005E_ED0F_7E57;
const CASES: usize = 20_000;

/// A random valid schedule with start and cliff anywhere in the bounds.
fn random_schedule(rng: &mut Rng) -> Schedule {
    let start = rng.range(0, 4_000_000_000) as i64;
    let duration = match rng.next() % 4 {
        0 => rng.range(1, 10),
        1 => MAX_DURATION_SECS as u64 - rng.range(0, 10),
        _ => rng.range(1, MAX_DURATION_SECS as u64),
    } as i64;
    let end = start + duration;
    let cliff = start + rng.range(0, duration as u64) as i64;
    Schedule::new(rng.total(), start, cliff, end).expect("generated parameters are valid")
}

fn sample_times(rng: &mut Rng, s: &Schedule, n: usize) -> Vec<i64> {
    let mut times: Vec<i64> = (0..n).map(|_| rng.range(s.start().saturating_sub(5).max(0) as u64, (s.end() + 5) as u64) as i64).collect();
    // always include the interesting boundaries
    times.extend([0, s.start(), s.cliff().saturating_sub(1), s.cliff(), s.end() - 1, s.end(), s.end() + 1, i64::MAX]);
    times.sort_unstable();
    times
}

#[test]
fn vested_is_monotonic_and_never_exceeds_total() {
    let mut rng = Rng(SEED);
    for case in 0..CASES {
        let s = random_schedule(&mut rng);
        let mut previous = 0u64;
        for t in sample_times(&mut rng, &s, 12) {
            let v = s.vested_at(t).expect("no overflow");
            assert!(v >= previous, "seed {SEED:#x} case {case}: vested fell from {previous} to {v} at {t} for {s:?}");
            assert!(v <= s.total(), "seed {SEED:#x} case {case}: vested {v} exceeds total for {s:?}");
            previous = v;
        }
    }
}

#[test]
fn nothing_vests_before_the_cliff_and_everything_at_the_end() {
    let mut rng = Rng(SEED ^ 1);
    for case in 0..CASES {
        let s = random_schedule(&mut rng);
        assert_eq!(s.vested_at(s.cliff() - 1), Ok(0), "seed case {case}: {s:?}");
        assert_eq!(s.vested_at(0), Ok(0), "case {case}: nothing has vested at the epoch");
        assert_eq!(s.vested_at(s.end()), Ok(s.total()), "case {case}: {s:?}");
        assert_eq!(s.vested_at(s.end() + 1_000), Ok(s.total()));
        if s.end() > s.cliff() {
            assert!(s.vested_at(s.end() - 1).expect("no overflow") <= s.total());
        }
    }
}

#[test]
fn rounding_never_overpays_and_is_the_exact_floor() {
    let mut rng = Rng(SEED ^ 2);
    for case in 0..CASES {
        let s = random_schedule(&mut rng);
        let duration = (s.end() - s.start()) as u128;
        for t in sample_times(&mut rng, &s, 8) {
            let v = u128::from(s.vested_at(t).expect("no overflow"));
            if t >= s.cliff() && t < s.end() {
                let elapsed = (t - s.start()) as u128;
                let exact_numerator = u128::from(s.total()) * elapsed;
                // v is not more than the exact amount ...
                assert!(v * duration <= exact_numerator, "case {case}: overpaid at {t} for {s:?}");
                // ... and is the largest integer with that property
                assert!((v + 1) * duration > exact_numerator, "case {case}: underpaid by more than rounding at {t} for {s:?}");
            }
        }
    }
}

#[test]
fn claims_over_time_sum_to_exactly_the_total() {
    let mut rng = Rng(SEED ^ 3);
    for case in 0..CASES / 4 {
        let s = random_schedule(&mut rng);
        let mut claimed = 0u64;
        let mut paid_sum = 0u128;
        let mut times = sample_times(&mut rng, &s, 10);
        times.retain(|t| *t < i64::MAX);
        // the beneficiary may claim at any of these times, but eventually claims at the end
        times.push(s.end());
        for t in times {
            let amount = s.claimable(t, claimed).expect("consistent state");
            if !rng.next().is_multiple_of(3) || t >= s.end() {
                claimed = s.after_claim(claimed, amount).expect("within total");
                paid_sum += u128::from(amount);
                assert_eq!(claimed, s.vested_at(t).expect("no overflow"), "case {case}: a full claim catches up with vested");
            }
            assert!(claimed <= s.total());
        }
        assert_eq!(paid_sum, u128::from(s.total()), "case {case}: claims sum to the total for {s:?}");
        assert_eq!(s.is_settled(claimed, 0), Ok(true));
    }
}

#[test]
fn repeated_claims_at_the_same_time_pay_nothing_more() {
    let mut rng = Rng(SEED ^ 4);
    for _ in 0..CASES / 4 {
        let s = random_schedule(&mut rng);
        let t = rng.range(s.start() as u64, s.end() as u64) as i64;
        let first = s.claimable(t, 0).expect("ok");
        let claimed = s.after_claim(0, first).expect("ok");
        assert_eq!(s.claimable(t, claimed), Ok(0));
    }
}

#[test]
fn revocation_conserves_every_token() {
    let mut rng = Rng(SEED ^ 5);
    for case in 0..CASES {
        let s = random_schedule(&mut rng);
        let t = sample_times(&mut rng, &s, 1)[(rng.next() % 9) as usize];
        let vested = s.vested_at(t).expect("ok");
        // any consistent claimed amount: at most what has vested
        let claimed = if vested == 0 { 0 } else { rng.range(0, vested) };
        let r = s.revocation(t, claimed).expect("split");
        assert_eq!(
            u128::from(claimed) + u128::from(r.to_beneficiary) + u128::from(r.to_authority),
            u128::from(s.total()),
            "case {case}: {s:?} at {t}"
        );
        assert_eq!(r.vested, vested);
        assert_eq!(s.is_settled(claimed + r.to_beneficiary, r.to_authority), Ok(true));
    }
}

#[test]
fn u64_edges_never_panic_and_stay_within_the_total() {
    let totals = [1u64, 2, 3, u64::MAX / 2, u64::MAX - 1, u64::MAX];
    let durations = [1i64, 2, 3, 86_400, MAX_DURATION_SECS - 1, MAX_DURATION_SECS];
    let starts = [0i64, 1, 1_700_000_000, 4_000_000_000];
    for &total in &totals {
        for &duration in &durations {
            for &start in &starts {
                let end = start + duration;
                for cliff in [start, start + duration / 2, end] {
                    let s = Schedule::new(total, start, cliff, end).expect("valid");
                    let mut previous = 0u64;
                    let mut times = vec![0, start - 1, start, start + 1, cliff - 1, cliff, cliff + 1, end - 1, end, end + 1, i64::MAX];
                    times.retain(|t| *t >= 0);
                    times.sort_unstable();
                    for t in times {
                        let v = s.vested_at(t).expect("no overflow at the edges");
                        assert!(v <= total);
                        assert!(v >= previous, "monotonic at the edges: {s:?} at {t}");
                        previous = v;
                    }
                    assert_eq!(s.vested_at(end), Ok(total));
                }
            }
        }
    }
}

/// Count-based oracle that shares no code with the division in `vested_at`: the vested amount is the number of
/// whole tokens k in 1..=total with k * duration <= total * elapsed.
fn oracle(total: u64, start: i64, cliff: i64, end: i64, now: i64) -> u64 {
    if now < cliff {
        return 0;
    }
    if now >= end {
        return total;
    }
    let (elapsed, duration) = ((now - start) as u128, (end - start) as u128);
    (1..=u128::from(total)).filter(|k| k * duration <= u128::from(total) * elapsed).count() as u64
}

#[test]
fn matches_a_count_based_oracle_on_every_small_schedule() {
    for total in 1..=14u64 {
        for duration in 1..=14i64 {
            for cliff_offset in 0..=duration {
                let (start, end) = (3, 3 + duration);
                let cliff = start + cliff_offset;
                let s = Schedule::new(total, start, cliff, end).expect("valid");
                for now in 0..(end + 3) {
                    assert_eq!(
                        s.vested_at(now),
                        Ok(oracle(total, start, cliff, end, now)),
                        "total {total} start {start} cliff {cliff} end {end} now {now}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_generator_is_deterministic() {
    let (mut a, mut b) = (Rng(SEED), Rng(SEED));
    for _ in 0..100 {
        assert_eq!(a.next(), b.next());
    }
    assert_ne!(Rng(SEED).next(), Rng(SEED + 1).next());
}
