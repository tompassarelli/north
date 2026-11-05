use super::*;
use crate::cli::cloud_land::land_handback;
use serde_json::json;
use std::cell::RefCell;

const EVENTS: &str = include_str!("../fixtures/cloud/handback-events.jsonl");
const NOW: f64 = 1_000_000.0;

fn launch(session: &str, at: f64) -> CloudLaunch {
    CloudLaunch { at, repo: "north".into(), session: session.into(), branch: format!("claude/land-{session}") }
}

/// The sessions API for one finished session that handed back and one still running.
fn api(calls: &RefCell<Vec<String>>) -> impl FnMut(&str, &str, Option<&str>) -> Result<(u16, String), String> + '_ {
    move |method, path, _| {
        calls.borrow_mut().push(format!("{method} {path}"));
        let events: Vec<Value> = EVENTS.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let body = match path {
            "/v1/sessions/session_done?beta=true" => json!({"id": "session_done", "session_status": "idle", "updated_at": "2026-11-05T10:00:00Z"}),
            "/v1/sessions/session_done/events?beta=true&limit=1000" => json!({"data": events, "has_more": false}),
            "/v1/sessions/session_busy?beta=true" => json!({"id": "session_busy", "session_status": "running", "updated_at": "2026-11-05T10:01:00Z"}),
            _ => return Err(format!("unexpected {method} {path}")),
        };
        Ok((200, body.to_string()))
    }
}

/// Every program `north cloud land` runs, answered as a clean landing would answer it.
fn programs<'a>(root: &'a Path, ran: &'a RefCell<Vec<String>>) -> impl FnMut(&Path, &str, &[&str]) -> Result<String, String> + 'a {
    move |_, program, args| {
        ran.borrow_mut().push(format!("{program} {}", args.join(" ")));
        Ok(match (program, args.first().copied()) {
            ("git", Some("rev-parse")) if args.contains(&"--git-common-dir") => format!("{}/north/main/.git\n", root.display()),
            ("git", Some("rev-parse")) => "0123456789abcdef0123456789abcdef01234567\n".into(),
            ("git", Some("log")) => "Install the fake agent's stop handler\n\0".into(),
            ("safe-push", _) => "Done: queued cloud-session_done\n".into(),
            _ => String::new(),
        })
    }
}

fn reap_once(root: &Path, launches: &[CloudLaunch], calls: &RefCell<Vec<String>>, ran: &RefCell<Vec<String>>) -> (Reaped, String) {
    let state = root.join("state");
    let mut run = programs(root, ran);
    let mut land = |_: &CloudLaunch, events: &[Value], transcript: &Path| land_handback(events, transcript, &root.join("north/main"), &state, &mut run);
    let mut out = Vec::new();
    let reaped = reap(launches, &mut api(calls), &state.join("reap"), NOW, &mut land, &mut out).unwrap();
    (reaped, String::from_utf8(out).unwrap())
}

fn count(ran: &RefCell<Vec<String>>, program: &str) -> usize {
    ran.borrow().iter().filter(|call| call.starts_with(&format!("{program} "))).count()
}

#[test]
fn reap_queues_a_finished_handback_once_across_runs_and_leaves_an_unfinished_session_alone() {
    let root = tempfile::tempdir().unwrap();
    let launches = [launch("session_done", NOW - 600.0), launch("session_busy", NOW - 60.0)];
    let (calls, ran) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));

    let (first, text) = reap_once(root.path(), &launches, &calls, &ran);
    assert_eq!(first, Reaped { landed: 1, failed: 0 }, "{text}");
    assert!(text.contains("session_done (north claude/land-session_done): ") && text.contains("Done: queued"), "{text}");
    assert!(text.contains("session_busy (north claude/land-session_busy): left alone, session running"), "{text}");
    assert_eq!(count(&ran, "safe-push"), 1);
    assert_eq!(count(&ran, "north land checks"), 1, "the lane's land checks ran once: {:?}", ran.borrow());
    assert!(!calls.borrow().iter().any(|call| call.contains("session_busy/events")), "{:?}", calls.borrow());

    calls.borrow_mut().clear();
    let (second, text) = reap_once(root.path(), &launches, &calls, &ran);
    assert_eq!(second, Reaped::default(), "{text}");
    assert_eq!(count(&ran, "safe-push"), 1, "a second run queues nothing: {:?}", ran.borrow());
    assert_eq!(*calls.borrow(), ["GET /v1/sessions/session_busy?beta=true"], "a landed session is never read again");

    std::fs::remove_file(root.path().join("state/reap/session_done.landed")).unwrap();
    let (third, text) = reap_once(root.path(), &launches, &calls, &ran);
    assert_eq!(third.landed, 1);
    assert!(text.contains("no-op: this handback was already queued"), "the landing's own record still refuses a second queue: {text}");
    assert_eq!(count(&ran, "safe-push"), 1);
}

#[test]
fn reap_skips_launches_outside_its_window_and_rereads_a_handbackless_session_only_once_it_changes() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let (calls, ran) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    let old = [launch("session_done", NOW - WINDOW_S - 1.0)];
    let (reaped, _) = reap_once(root.path(), &old, &calls, &ran);
    assert_eq!(reaped, Reaped::default());
    assert!(calls.borrow().is_empty());

    let mut lands = 0;
    let mut none = |_: &CloudLaunch, _: &[Value], _: &Path| {
        lands += 1;
        None
    };
    let recent = [launch("session_done", NOW)];
    for _ in 0..2 {
        let mut out = Vec::new();
        reap(&recent, &mut api(&calls), &state, NOW, &mut none, &mut out).unwrap();
    }
    assert_eq!(lands, 1, "an unchanged session that had no handback is not reread");
    assert_eq!(std::fs::read_to_string(state.join("session_done.seen")).unwrap().trim(), "2026-11-05T10:00:00Z");
}
