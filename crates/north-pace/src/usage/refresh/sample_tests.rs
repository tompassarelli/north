use std::fs;

use rusqlite::Connection;
use serde_json::json;

use super::claude::{self, Settings};
use super::sample::{Ports, run};
use crate::foundation::clock::Timestamp;
use crate::usage::profiles;

const HEADERS: &str = "HTTP/2 200 \r\nAnthropic-Ratelimit-Unified-5h-Status: allowed_warning\r\nanthropic-ratelimit-unified-5h-utilization: 0.37\r\nanthropic-ratelimit-unified-5h-reset: 1793118000\r\nanthropic-ratelimit-unified-7d-status: allowed\r\nanthropic-ratelimit-unified-7d-utilization: 0.0449\r\nanthropic-ratelimit-unified-7d-reset: 1793692800\r\nanthropic-ratelimit-unified-representative-claim: five_hour\r\n\r\n";

fn cells(db: &Connection, sql: &str) -> Vec<(String, String)> {
    db.prepare(sql).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

fn pair(first: &str, second: &str) -> (String, String) {
    (first.to_owned(), second.to_owned())
}

#[test]
fn a_sample_records_token_header_reads_with_their_status_and_the_extra_usage_flag_of_a_login() {
    let home = tempfile::tempdir().unwrap();
    let fetched = Timestamp::now().unix_millis();
    fs::write(
        home.path().join(".claude.json"),
        json!({"oauthAccount": {"accountUuid": "login"}, "cachedUsageUtilization": {"accountUuid": "login", "fetchedAtMs": fetched,
            "utilization": {"limits": [{"kind": "weekly_all", "group": "weekly", "percent": 12, "resets_at": "2099-01-05T00:00:00+00:00"}],
                "extra_usage": {"is_enabled": true, "utilization": 3.0}, "cedar_ember": {"grants": []}}}})
        .to_string(),
    )
    .unwrap();
    let settings = Settings { cfg: json!({}), host: "fixture".into(), state: home.path().join("state"), home: home.path().to_path_buf(), configured: None, refresh: false };
    let cli = || claude::read(&settings);
    let headers = vec![("d".to_owned(), Ok(HEADERS.to_owned())), ("gone".to_owned(), Err("no token".to_owned()))];
    let ports = Ports { headers, cli: &cli, ingest: &|_| Err("unexpected".into()), at: 1_793_100_000.0 };
    let db = Connection::open_in_memory().unwrap();
    run(&db, false, &ports).unwrap();
    let header = cells(&db, "SELECT name, CAST(used_pct AS TEXT) || ' ' || resets_at FROM usage WHERE account = 'd' AND kind = 'window' AND source LIKE '%headers%' ORDER BY name");
    assert_eq!(header, [pair("session", "37.0 2026-10-27T16:20:00Z"), pair("weekly_all", "4.0 2026-11-03T08:00:00Z")]);
    let status = cells(&db, "SELECT name, substr(source, instr(source, 'status=')) FROM usage WHERE account = 'd' AND kind = 'status' ORDER BY name");
    assert_eq!(status, [pair("session", "status=allowed_warning"), pair("weekly_all", "status=allowed")]);
    let hashed = profiles::short_hash("login");
    let extra = cells(&db, &format!("SELECT name, CAST(amount AS TEXT) FROM usage WHERE account = '{hashed}' AND kind = 'billing' AND name = 'extra_usage'"));
    assert_eq!(extra, [pair("extra_usage", "1.0")]);
    assert_eq!(cells(&db, "SELECT account, kind FROM usage WHERE account = 'gone'"), []);
}

#[test]
fn a_second_sample_of_the_same_readings_adds_nothing() {
    let ports = Ports { headers: vec![("d".to_owned(), Ok(HEADERS.to_owned()))], cli: &Vec::new, ingest: &|_| Err("unexpected".into()), at: 1_793_100_000.0 };
    let db = Connection::open_in_memory().unwrap();
    run(&db, false, &ports).unwrap();
    let count = || db.query_row("SELECT count(*) FROM usage", [], |row| row.get::<_, i64>(0)).unwrap();
    let first = count();
    run(&db, false, &ports).unwrap();
    assert_eq!((first > 0, count()), (true, first));
}
