//! Prints the vesting curve of an example schedule and what a revocation would split.
//! Run: `cargo run -p vesting_core --example curve`
#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use vesting_core::Schedule;

const DAY: i64 = 86_400;

fn main() {
    // 1,000,000 tokens over 30 days, with a cliff after 7 days
    let start = 1_700_000_000;
    let schedule = Schedule::new(1_000_000, start, start + 7 * DAY, start + 30 * DAY).expect("valid schedule");
    println!(
        "schedule: total {} tokens, start {}, cliff {} (day 7), end {} (day 30)",
        schedule.total(),
        schedule.start(),
        schedule.cliff(),
        schedule.end()
    );
    println!();
    println!("{:>4}  {:>10}  {:>8}  {:>18}", "day", "vested", "percent", "claimable (claims at days 7, 15)");
    let mut claimed = 0;
    for day in [0, 3, 6, 7, 8, 10, 14, 15, 20, 29, 30, 31] {
        let now = start + day * DAY;
        let vested = schedule.vested_at(now).expect("no overflow");
        let claimable = schedule.claimable(now, claimed).expect("consistent");
        println!("{day:>4}  {vested:>10}  {:>7.2}%  {claimable:>18}", 100.0 * vested as f64 / schedule.total() as f64);
        if day == 7 || day == 15 {
            claimed = schedule.after_claim(claimed, claimable).expect("within total");
        }
    }
    println!();
    println!("a revocation at day 20 after claims at days 7 and 15:");
    let at = start + 20 * DAY;
    let claimed = schedule.vested_at(start + 15 * DAY).expect("no overflow");
    let split = schedule.revocation(at, claimed).expect("split");
    println!(
        "  vested {}  already claimed {}  to the beneficiary {}  back to the authority {}",
        split.vested, claimed, split.to_beneficiary, split.to_authority
    );
    println!(
        "  check: {} + {} + {} = {}",
        claimed,
        split.to_beneficiary,
        split.to_authority,
        claimed + split.to_beneficiary + split.to_authority
    );
    println!();
    println!("rounding: 10 tokens over 3 seconds vest as {:?} (floor, never more than the exact amount)", {
        let s = Schedule::new(10, 0, 0, 3).expect("valid");
        [1, 2, 3].map(|t| s.vested_at(t).expect("no overflow"))
    });
}
