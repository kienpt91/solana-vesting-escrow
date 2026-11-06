//! A lifecycle on the in-process bank, printed step by step: create, early claim refused, claim at the cliff, claim later,
//! revoke, close. Run: `cargo run -p vesting_tests --example walkthrough` (the container wrapper in `scripts/rs.sh` sets it up).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use vesting_tests::{Scenario, Setup, DAY};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // 1,000 tokens (6 decimals) over 30 days, cliff after 7 days, revocable
    let mut s = Scenario::new(Setup {
        total: 1_000_000_000,
        creator_balance: 1_000_000_000,
        start: 0,
        cliff: 7 * DAY,
        end: 30 * DAY,
        ..Setup::default()
    })
    .await;
    println!("program   {}", s.program_id());
    println!("schedule  {}", s.schedule);
    println!("vault     {}", s.vault);
    println!("created: vault holds {} base units", s.vault_balance().await);

    s.at(6 * DAY).await;
    println!("day 6  claim -> {}", s.claim().await.map_or_else(|f| f.describe(), |()| "paid".to_string()));
    s.at(7 * DAY).await;
    s.claim().await.unwrap();
    println!("day 7  claim -> paid, beneficiary holds {}", s.beneficiary_balance().await);
    s.at(15 * DAY).await;
    s.claim().await.unwrap();
    println!("day 15 claim -> paid, beneficiary holds {}", s.beneficiary_balance().await);
    s.at(20 * DAY).await;
    s.revoke().await.unwrap();
    println!(
        "day 20 revoke -> beneficiary holds {}, authority holds {}, vault holds {}",
        s.beneficiary_balance().await,
        s.authority_balance().await,
        s.vault_balance().await
    );
    println!("day 21 claim after revoke -> {}", s.claim().await.map_or_else(|f| f.describe(), |()| "paid".to_string()));
    s.close().await.unwrap();
    println!(
        "close -> schedule account exists: {}, vault exists: {}",
        s.env.account(&s.schedule).await.is_some(),
        s.env.account(&s.vault).await.is_some()
    );
}
