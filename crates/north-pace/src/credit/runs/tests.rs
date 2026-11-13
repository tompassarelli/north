use super::*;

const MODEL: &str = "claude-opus-5-5";
const DAY_MS: i64 = 86_400_000;
const NOW: i64 = 20_000 * DAY_MS;

fn tokens(output: i64) -> TokenUse {
    TokenUse { output, ..TokenUse::default() }
}

fn run(session: &str, started_ms: i64, ended_ms: i64, output: i64) -> Finished {
    Finished { session: session.to_owned(), account: Some("api:proton".to_owned()), model: Some(MODEL.to_owned()), started_ms, ended_ms, tokens: tokens(output) }
}

fn turn(session: &str, output: i64) -> Open {
    Open { session: session.to_owned(), account: "api:proton".to_owned(), model: Some(MODEL.to_owned()), tokens: tokens(output) }
}

fn snapshot(host: &str, read_ms: i64, finished: Vec<Finished>, open: Vec<Open>) -> Snapshot {
    Snapshot { host: host.to_owned(), read_ms, finished, open }
}

fn proton(reading: &Reading) -> f64 {
    reading.orgs.get("proton").map_or(0.0, |spends| spends.iter().map(|(_, usd)| usd).sum())
}

fn usd(output: i64) -> f64 {
    PriceTable::load().unwrap().price(MODEL).unwrap().of(tokens(output))
}

#[test]
fn a_run_another_host_recorded_is_charged_once_however_many_hosts_report_it() {
    let prices = PriceTable::load().unwrap();
    let local = vec![run("north-1", NOW - 5000, NOW - 4000, 1_000_000)];
    let nexus = snapshot("nexus", NOW - 1000, vec![run("north-634-d0216", NOW - 3000, NOW - 2000, 2_000_000), run("north-1", NOW - 5000, NOW - 4000, 1_000_000)], Vec::new());
    let gex45 = snapshot("gex45", NOW - 1000, vec![run("north-634-d0216", NOW - 3000, NOW - 2000, 2_000_000)], Vec::new());
    let reading = merge(local, vec![nexus, gex45], &prices, NOW);
    assert!((proton(&reading) - usd(3_000_000)).abs() < 1e-9, "{reading:?}");
}

#[test]
fn a_turn_still_open_is_charged_until_a_run_of_its_session_ends_after_the_snapshot() {
    let prices = PriceTable::load().unwrap();
    let open = || snapshot("nexus", NOW - 60_000, Vec::new(), vec![turn("north-634-d0216", 500_000)]);
    assert!((proton(&merge(Vec::new(), vec![open()], &prices, NOW)) - usd(500_000)).abs() < 1e-9);
    let finished = vec![run("north-634-d0216", NOW - 90_000, NOW - 10_000, 700_000)];
    assert!((proton(&merge(finished, vec![open()], &prices, NOW)) - usd(700_000)).abs() < 1e-9);
    let stale = snapshot("nexus", NOW - 3_600_000, Vec::new(), vec![turn("north-634-d0216", 500_000)]);
    assert_eq!(proton(&merge(Vec::new(), vec![stale], &prices, NOW)), 0.0);
}

#[test]
fn runs_another_host_recorded_on_any_account_raise_the_fleets_last_day_of_spend() {
    let prices = PriceTable::load().unwrap();
    let plan = |session: &str, started_ms: i64, output: i64| Finished { account: Some("claude:max".to_owned()), ..run(session, started_ms, started_ms + 1000, output) };
    let local = vec![plan("north-1", NOW - 5000, 1_000_000)];
    let here = merge(local.clone(), Vec::new(), &prices, NOW).last_day_usd;
    let gex45 = snapshot("gex45", NOW - 1000, vec![plan("north-752-d0321", NOW - 3000, 2_000_000), plan("north-1", NOW - 5000, 1_000_000), plan("north-old", NOW - 2 * DAY_MS, 4_000_000)], Vec::new());
    let fleet = merge(local, vec![gex45], &prices, NOW);
    assert!((here - usd(1_000_000)).abs() < 1e-9 && (fleet.last_day_usd - usd(3_000_000)).abs() < 1e-9, "{fleet:?}");
    assert!(fleet.orgs.is_empty(), "{fleet:?}");
}

#[test]
fn a_run_the_ledger_recorded_without_an_account_does_not_fail_the_snapshot() {
    let dir = std::env::temp_dir().join(format!("north-845-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ledger = dir.join("ledger.db");
    let db = Connection::open(&ledger).unwrap();
    db.execute_batch("CREATE TABLE runs(session TEXT NOT NULL, account TEXT, model TEXT, started_ms INTEGER NOT NULL, ended_ms INTEGER NOT NULL, tokens_in INTEGER NOT NULL, tokens_out INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL, tokens_cache_write INTEGER NOT NULL)").unwrap();
    db.execute("INSERT INTO runs VALUES ('killed-before-its-first-turn', NULL, ?1, ?2, ?3, 0, 0, 0, 0)", rusqlite::params![MODEL, NOW - 5000, NOW - 4000]).unwrap();
    drop(db);
    let taken = take_snapshot("whiterabbit", &ledger, Vec::new(), NOW);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(taken.unwrap().finished.iter().map(|run| run.account.clone()).collect::<Vec<_>>(), vec![None]);
}
