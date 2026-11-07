use super::{Org, Reading, declared, moved_to, open, ran_out, settle, since, switch, today};
use crate::credit::{Grant, launchable};
use crate::fleet::model::Transcript;
use crate::fleet::runtime::Runtime;
use crate::providers::claude::fleet::Claude;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const POLICY: &str = "[accounts.claude.api_orgs.proton]\nid = \"p\"\nkeys = { gex45 = [\"/k/p\"] }\n[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = { gex45 = [\"/k/g\"] }\n";

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, expires: expires.to_owned() }
}

// KEEP(bug): an api org whose first grant is spent still ranked by that grant's date, so a later grant was drawn before an untouched sooner one (north#737)
// KEEP(bug): an org's sessions failed their turns on the API's low-credit refusal instead of moving to the next org (north#755)
#[test]
fn the_pool_opens_the_org_whose_soonest_grant_with_credit_left_expires_first() {
    let proton = [grant(199.99, "2026-11-08"), grant(200.0, "2026-12-08")];
    let spent = [("2026-11-06".to_owned(), 150.0), ("2026-11-07".to_owned(), 66.23)];
    let today = "2026-11-07";
    let open_grants: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), launchable(&proton, &spent, today)), ("g".to_owned(), launchable(&[grant(200.0, "2026-11-12")], &[], today))].into();
    let orgs = declared(&POLICY.parse().unwrap(), &open_grants, Path::new("/home"), "gex45");
    let first = |orgs: &[Org]| open(orgs, today, |_| false, |_| true).map(|(org, _)| org.name.clone());
    assert_eq!(first(&orgs), Some("gmail".to_owned()));
    let all_spent: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), Vec::new()), ("g".to_owned(), Vec::new())].into();
    assert_eq!(first(&declared(&POLICY.parse().unwrap(), &all_spent, Path::new("/home"), "gex45")), None);

    let at = |usd: f64| launchable(&proton, &[(today.to_owned(), usd)], today);
    assert_eq!(at(199.98).len(), 2);
    assert_eq!(at(199.99), [grant(200.0, "2026-12-08")]);
    assert_eq!(at(1000.0), [grant(200.0, "2026-12-08")]);
    let mut transcript = Transcript::default();
    for frame in [
        json!({"type": "user", "uuid": "u1", "message": {"content": "Item: north#722"}}),
        json!({"type": "assistant", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Credit balance is too low"}]}}),
        json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"}),
    ] {
        Claude.observe(&frame, &mut transcript);
    }
    assert!(ran_out(&transcript.entries));
    assert_eq!(moved_to("api:proton", &orgs, today, |_| false).as_deref(), Some("api:gmail"));
    assert_eq!(moved_to("api:proton", &orgs, today, |org| org.name == "gmail"), None);
}

fn append(stream: &Path, frames: &[serde_json::Value]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(stream).unwrap();
    for frame in frames {
        writeln!(file, "{frame}").unwrap();
    }
}

fn low_credit_turn(uuid: &str) -> [serde_json::Value; 3] {
    [
        json!({"type": "user", "uuid": uuid, "session_id": "c", "message": {"content": "carry on"}}),
        json!({"type": "assistant", "session_id": "c", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Credit balance is too low"}]}}),
        json!({"type": "result", "session_id": "c", "subtype": "success", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"}),
    ]
}

// KEEP(bug): a session moved off an org that ran out of credit had its old low-credit turn read again on the next tick, which marked the new org exhausted while it still had credit; and a message delivered after a failed turn hid the failure, so no move happened (north#755)
#[test]
fn only_the_newest_low_credit_turn_on_the_current_account_counts() {
    let scratch = tempfile::tempdir().unwrap();
    let stream = scratch.path().join("stream.jsonl");
    let ran_out_from = |offset: u64| ran_out(&since(&Claude, &stream, offset, Some("c")).entries);
    append(&stream, &low_credit_turn("gmail"));
    append(&stream, &[json!({"type": "result", "session_id": "other", "subtype": "success", "result": "done"})]);
    assert!(ran_out_from(0));
    let moved = std::fs::metadata(&stream).unwrap().len();
    assert!(!ran_out_from(moved));
    append(&stream, &[json!({"type": "user", "uuid": "report", "session_id": "c", "message": {"content": "[from child] Done: landed"}})]);
    assert!(ran_out_from(0));
    assert!(!ran_out_from(moved));
    append(&stream, &low_credit_turn("apple")[1..]);
    assert!(ran_out_from(moved));
}

// KEEP(bug): on 2026-11-07 the pool marked an org exhausted from its counted spend, a 10% reserve and a billing multiplier while the provider still held credit on it (north#792)
#[test]
fn a_tick_keeps_an_org_open_and_its_sessions_on_it_whatever_its_counted_spend_until_the_provider_refuses_a_turn() {
    use crate::fleet::registry::{Fleet, fixtures};
    let tick = |spent: f64| {
        let home = tempfile::tempdir().unwrap();
        let here = crate::platform::host::name();
        let policy = home.path().join("orchestration.toml");
        std::fs::write(&policy, format!("[accounts.claude.api_orgs.proton]\nid = \"p\"\nkeys = {{ \"{here}\" = [\"/k/p\"] }}\n[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = {{ \"{here}\" = [\"/k/g\"] }}\n")).unwrap();
        let fleet = Fleet::at(home.path().into(), crate::fleet::runtimes::Runtimes::new(vec![std::sync::Arc::new(Claude)]), std::sync::Arc::new(crate::fleet::fakes::Paper::default())).unwrap();
        for name in ["idle", "working"] {
            fixtures::live(&fleet, name);
            let file = home.path().join(".local/state/north/fleet").join(name).join("session.json");
            let mut record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            record["account"] = json!("api:proton");
            std::fs::write(&file, record.to_string()).unwrap();
        }
        let prompt = json!({"type": "user", "session_id": "working", "uuid": "p1", "message": {"content": "split the crate"}});
        std::fs::write(home.path().join(".local/state/north/fleet/working/g1/stream.jsonl"), format!("{prompt}\n")).unwrap();
        let today = today();
        let proton = launchable(&[grant(200.0, "2099-12-31")], &[(today.clone(), spent)], &today);
        let reading = Reading { grants: [("p".to_owned(), proton), ("g".to_owned(), vec![grant(200.0, "2099-12-31")])].into(), next_credits: [("p".to_owned(), "2099-01-01".to_owned())].into(), today };
        let mut moved = Vec::new();
        let lines = settle(&fleet, &Claude, (&policy, home.path()), &reading, |record, spent, to, _, failed| {
            moved.push((record.name.clone(), spent.to_owned(), to.to_owned(), failed));
            String::new()
        });
        (moved, std::fs::read_to_string(home.path().join(".local/state/north/api-exhausted/p")).ok(), lines)
    };

    for spent in [150.0, 199.99, 400.0] {
        let (moved, mark, lines) = tick(spent);
        assert!(moved.is_empty() && mark.is_none(), "{spent}: {moved:?} {mark:?} {lines:?}");
    }
}

fn host_process() -> u32 {
    use std::os::unix::process::CommandExt;
    let mut host = std::process::Command::new("sleep").arg("60").process_group(0).spawn().unwrap();
    let pid = host.id();
    std::thread::spawn(move || host.wait());
    pid
}

// KEEP(bug): at 12:42 PM on 2026-11-07 n-replay-case, idle with its last turn ended success and a background task open, was moved onto another org and its parent was told its host exited mid-task; the move's halt was recorded as a relaunch under another model or effort (north#781)
#[test]
fn a_session_moved_between_accounts_raises_no_mid_task_exit_and_no_carry_on_and_its_halt_says_it_moved() {
    use crate::fleet::registry::{Fleet, fixtures};
    let home = tempfile::tempdir().unwrap();
    let fleet = Fleet::at(home.path().into(), crate::fleet::runtimes::Runtimes::new(vec![std::sync::Arc::new(Claude)]), std::sync::Arc::new(crate::fleet::fakes::Paper::default())).unwrap();
    let parent = fixtures::live(&fleet, "proxy");
    let child = fixtures::live(&fleet, "n-replay-case");
    let dir = home.path().join(".local/state/north/fleet/n-replay-case");
    let first = host_process();
    let mut record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
    (record["host_pid"], record["parent"], record["account"]) = (json!(first), json!("proxy"), json!("api:proton"));
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    std::fs::write(dir.join("host.pid"), first.to_string()).unwrap();
    let turn = [
        json!({"type": "user", "session_id": "n-replay-case", "uuid": "u1", "message": {"content": "replay the case"}}),
        json!({"type": "system", "subtype": "task_started", "session_id": "n-replay-case", "task_id": "b1"}),
        json!({"type": "assistant", "session_id": "n-replay-case", "message": {"content": [{"type": "text", "text": "Not done: the replay runs in the background.\nStop: capacity"}]}}),
        json!({"type": "result", "subtype": "success", "session_id": "n-replay-case"}),
    ];
    std::fs::write(dir.join("g1/stream.jsonl"), turn.map(|frame| frame.to_string() + "\n").concat()).unwrap();
    let watcher = fleet.clone();
    let watchdog = std::thread::spawn(move || watcher.guard("n-replay-case"));
    let moving = fleet.session("n-replay-case").unwrap();

    switch(&fleet, &moving, "api:gmail", |name| {
        let second = host_process();
        let mut stopped = fleet.session(name)?;
        stopped.host_pid = Some(second);
        crate::fleet::records::write_record(&dir, &stopped)?;
        std::fs::write(dir.join("host.pid"), second.to_string())?;
        fleet.phases()?.live(stopped.launch(), Some(second), "a session host started for it").map(drop)
    })
    .unwrap();
    watchdog.join().unwrap().unwrap();

    let told = parent.frames.lock().unwrap();
    assert!(told.iter().all(|frame| !frame.contains("exited mid-task")), "{told:?}");
    let handed = child.frames.lock().unwrap();
    assert!(handed.iter().all(|frame| !frame.contains("Carry on")), "{handed:?}");
    let ledger = rusqlite::Connection::open(home.path().join(".local/state/north/ledger.db")).unwrap();
    let cause: String = ledger.query_row("SELECT cause FROM lifecycle_moves WHERE instance LIKE 'n-replay-case@%' AND to_state = 'stopped'", [], |row| row.get(0)).unwrap();
    assert_eq!(cause, "moved onto another account");
}
