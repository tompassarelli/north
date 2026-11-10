use std::cell::RefCell;

use serde_json::Value;

use super::{COST, Pooled, USAGE, poll_org};
use north_pace::credit::readings::{Source, directory, get};
use north_pace::credit::{sources, standing};
use north_pace::foundation::clock::Timestamp;
use north_pace::router::pricing::PriceTable;

const AT: i64 = 1_794_286_800;
const KEY: &str = "fixture-admin-key-not-a-secret";

fn place(dir: &std::path::Path) {
    std::fs::write(
        dir.join("orchestration.toml"),
        "[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = { whiterabbit = [\"/run/secrets/anthropic-gmail-admin-key\"] }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("claude-accounts.toml"),
        "[[account]]\n[account.api_org]\nid = \"g\"\ngrants = [\n  { usd = 200.00, granted = 2026-11-06, expires = 2026-11-12 },\n  { usd = 200.00, granted = 2026-11-10, expires = 2026-12-10 },\n]\n",
    )
    .unwrap();
}

#[test]
fn the_admin_reports_price_each_day_and_record_the_grants_left_over_norths_estimate() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let keys = dir.path().join("secrets");
    std::fs::create_dir_all(&keys).unwrap();
    let key = keys.join("anthropic-gmail-admin-key");
    std::fs::write(&key, format!("{KEY}\n")).unwrap();
    let state = dir.path().join("state");
    let org = Pooled { name: "gmail".to_owned(), id: "g".to_owned(), key };
    let seen = RefCell::new(Vec::new());
    let fetch = |path: &str, params: &[(&str, String)]| {
        seen.borrow_mut().push((path.to_owned(), params.iter().map(|(name, value)| ((*name).to_owned(), value.clone())).collect::<Vec<_>>()));
        let text = match path {
            USAGE => include_str!("fixtures/usage_report_messages.json"),
            COST => include_str!("fixtures/cost_report.json"),
            other => panic!("unexpected report {other}"),
        };
        Ok(serde_json::from_str::<Value>(text).unwrap())
    };
    let now = Timestamp::from_unix_seconds(AT);
    let lines = poll_org(&dir.path().join("orchestration.toml"), &state, &org, &PriceTable::load().unwrap(), now, &fetch);

    let seen = seen.into_inner();
    let usage = seen.iter().find(|(path, _)| path == USAGE).expect("the usage report was not read");
    assert!(usage.1.iter().any(|(name, value)| name == "bucket_width" && value == "1h"), "{usage:?}");
    assert!(usage.1.iter().any(|(name, value)| name == "group_by[]" && value == "model"), "{usage:?}");
    assert!(usage.1.iter().any(|(name, value)| name == "starting_at" && value == "2026-11-06T00:00:00Z"), "the report starts at the earliest grant: {usage:?}");
    let cost = seen.iter().find(|(path, _)| path == COST).expect("the cost report was not read");
    assert!(cost.1.iter().any(|(name, value)| name == "bucket_width" && value == "1d"), "{cost:?}");

    let line = &lines[0];
    assert!(line.contains("reads $280.00 left from the Admin API"), "{line}");
    assert!(line.contains("spend $120.00 of $400.00 granted"), "{line}");
    assert!(line.contains("usage priced $42.00"), "{line}");
    assert!(line.contains("cost report $100.00 on 1 posted day (+78.00)"), "{line}");
    assert!(line.contains("unpriced claude-haiku-4-5-20251001"), "{line}");
    assert!(!line.contains(KEY), "the key reached the poll's output: {line}");

    let reading = get(&directory(&state), "g").expect("no provider reading was recorded");
    assert_eq!((reading.usd, reading.source), (280.0, Source::Provider));
    let org_sources = sources(&standing(&dir.path().join("orchestration.toml"), &state, now));
    let gmail = org_sources.iter().find(|source| source.credit.name == "gmail").unwrap();
    assert!(gmail.line(AT).contains("$280.00 left (provider reading 2026-11-10T05:00Z, estimate $400.00)"), "{}", gmail.line(AT));
}
