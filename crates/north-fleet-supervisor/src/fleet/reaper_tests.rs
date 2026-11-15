use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::gates::{GateBook, GateRequest};
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::registry::fixtures::live;
use crate::fleet::runtime::now_ms;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::records::ARCHIVE;
use crate::fleet::waits::Run;

const HOUR: Duration = Duration::from_secs(3600);

#[derive(Default)]
struct Sight {
    landed: BTreeSet<String>,
    running: BTreeMap<String, Work>,
    lanes: BTreeMap<String, Vec<north_base::land_ticket::Lane>>,
    lanes_ahead: BTreeMap<String, Unlanded>,
}

impl Evidence for Sight {
    fn landed(&self, _cwd: &Path, sha: &str) -> bool {
        self.landed.contains(sha)
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        records.iter().filter_map(|record| Some((record.name.clone(), *self.running.get(&record.name)?))).collect()
    }

    fn lanes(&self) -> BTreeMap<String, Vec<north_base::land_ticket::Lane>> {
        self.lanes.clone()
    }

    fn lane(&self, record: &SessionRecord) -> Option<Unlanded> {
        self.lanes_ahead.get(&record.name).cloned()
    }
}

fn fleet(home: &Path) -> (Fleet, Arc<Paper>) {
    let paper = Arc::new(Paper::default());
    (Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Stream)]), paper.clone()).unwrap().arming(|_| Ok(())), paper)
}

fn said(name: &str, kind: &str, text: &str) -> Value {
    json!({"type": kind, "session_id": name, "message": {"content": [{"type": "text", "text": text}]}})
}

fn parked(fleet: &Fleet, name: &str, parent: Option<&str>, frames: &[Value]) {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "parent": parent, "host_dir": dir, "stream_offset": 0, "task_ref": "north#636"});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let lines: String = frames.iter().map(|frame| format!("{frame}\n")).collect();
    std::fs::write(dir.join("g1/stream.jsonl"), lines).unwrap();
}

fn reported(fleet: &Fleet, name: &str, parent: Option<&str>, report: &str) {
    let result = json!({"type": "result", "subtype": "success", "session_id": name});
    parked(fleet, name, parent, &[said(name, "user", "Item: north#636"), said(name, "assistant", report), result]);
}

fn retired(reaped: &Reaped) -> Vec<&str> {
    reaped.retired.iter().map(|retired| retired.name.as_str()).collect()
}

fn archived_dir(fleet: &Fleet, name: &str) -> std::path::PathBuf {
    std::fs::read_dir(fleet.root.join(ARCHIVE)).unwrap().flatten().map(|entry| entry.path()).find(|path| path.file_name().unwrap().to_string_lossy().starts_with(&format!("{name}-"))).unwrap()
}

/// Eight workers parked under the coordinator's hold were retired on 2026-11-12 after a taken Not done report, lanes ahead of main and their parent untold (north#1235).
#[test]
fn a_worker_parked_on_an_unlanded_lane_after_a_not_done_report_stays_and_stale_retirements_tell_their_parent_every_lane_in_one_digest() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let lead = live(&fleet, "lead");
    reported(&fleet, "held", Some("lead"), "Not done: the commit is on the lane but the coordinator holds hand-ins.\nStop: landing");
    paper.claim_receipt("held", "turn:1", now_ms()).unwrap();
    reported(&fleet, "unlanded", Some("lead"), "Done: the fix is on the lane.");
    paper.claim_receipt("unlanded", "turn:1", now_ms()).unwrap();
    for name in ["exited", "slow"] {
        reported(&fleet, name, Some("lead"), "Done: nothing to land.");
        let record = fleet.root.join(name).join("session.json");
        let mut stopped: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        stopped["stopped"] = json!(true);
        std::fs::write(&record, stopped.to_string()).unwrap();
    }
    let lane = |path: &str, tip: &str, commits| Unlanded::Ahead(Ahead { path: path.into(), tip: tip.into(), commits });
    let lanes_ahead = [("held".to_owned(), lane("/home/tom/code/north/worktrees/held-lane", "8ffd6add62", 1)), ("exited".to_owned(), lane("/home/tom/code/north/worktrees/exited-lane", "6d0cecb063", 2)), ("unlanded".to_owned(), lane("/home/tom/code/north/worktrees/unlanded-lane", "1f2e3d4c5b", 1)), ("slow".to_owned(), Unlanded::Unread("/home/tom/code/north/worktrees/slow-lane".into()))];
    let evidence = Sight { lanes_ahead: lanes_ahead.into(), ..Sight::default() };

    let waiting = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
    assert!(retired(&waiting).is_empty() && fleet.root.join("held").exists() && fleet.root.join("unlanded").exists(), "a Done with a lane ahead of main keeps its session: {waiting:?}");
    let stale = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);

    let mut gone = retired(&stale);
    gone.sort_unstable();
    assert_eq!(gone, ["exited", "held", "unlanded"]);
    assert!(fleet.session("slow").is_ok(), "a lane probe that timed out keeps its session");
    let frames = lead.frames.lock().unwrap();
    let digests: Vec<&String> = frames.iter().filter(|frame| frame.contains("lanes hold commits not on origin/main")).collect();
    assert_eq!(digests.len(), 1, "one digest per parent and tick: {frames:?}");
    assert!(digests[0].contains("4 sessions") && digests[0].contains("held: retired; /home/tom/code/north/worktrees/held-lane at 8ffd6add62, 1 commits") && digests[0].contains("exited: retired; /home/tom/code/north/worktrees/exited-lane at 6d0cecb063, 2 commits") && digests[0].contains("slow: kept; its lane /home/tom/code/north/worktrees/slow-lane did not answer"), "{digests:?}");
}

/// On 2026-11-13 the stale-session sweep retired a few sessions per reaper tick and the coordinator was woken every few minutes by one-line digests (north#1235).
#[test]
fn retirement_digests_reach_a_parent_at_most_once_per_half_hour_the_first_at_once_and_the_rest_accumulated_across_ticks_and_a_restart_and_held_rows_arrive_as_one_count_line_with_the_read_verb() {
    let home = tempfile::tempdir().unwrap();
    let (before, paper) = fleet(home.path());
    let lead = live(&before, "lead");
    let minute = 60_000;
    let start = now_ms();
    let lane = |name: &str| (name.to_owned(), Unlanded::Ahead(Ahead { path: format!("/home/tom/code/north/worktrees/{name}-lane").into(), tip: "8ffd6add62".into(), commits: 1 }));
    let evidence = Sight { lanes_ahead: ["first", "second", "third"].map(lane).into(), ..Sight::default() };
    let digests = || lead.frames.lock().unwrap().iter().filter(|frame| frame.contains("lanes hold commits not on origin/main")).cloned().collect::<Vec<String>>();
    let retire = |fleet: &Fleet, paper: &Paper, name: &str, at: u64| {
        reported(fleet, name, Some("lead"), "Done: nothing to land.");
        paper.claim_receipt(name, "turn:1", now_ms()).unwrap();
        assert_eq!(retired(&reap_after(fleet, &evidence, Duration::ZERO, at, true)), [name]);
    };

    for (index, text) in ["alarm: a.service failed", "alarm: b.service failed", "alarm: c.service failed", "alarm: d.service failed", "cleared: /tmp/a ok", "cleared: /tmp/b ok", "cleared: /tmp/c ok", "disk is full", "disk is full", "disk is full", "disk is full", "disk is full"].iter().enumerate() {
        crate::fleet::held::hold(&before.root, "lead", "fleet", text, start + index as u64);
    }
    retire(&before, &paper, "first", start);
    let sent = digests();
    assert!(sent.len() == 1 && sent[0].contains("1 sessions") && sent[0].contains("first: retired"), "{sent:?}");
    let held: Vec<String> = lead.frames.lock().unwrap().iter().filter(|frame| frame.contains("held messages")).cloned().collect();
    assert_eq!(held.len(), 1, "{held:?}");
    let body: serde_json::Value = serde_json::from_str(&held[0]).unwrap();
    let text = body["message"]["content"].as_str().unwrap();
    assert!(text.contains("[north: 12 held messages") && text.contains("`north fleet held --since 1m`") && text.lines().count() == 1, "{held:?}");
    assert_eq!(crate::fleet::held::since(&before.root, 0).len(), 12);

    retire(&before, &paper, "second", start + 5 * minute);
    let (restarted, paper) = fleet(home.path());
    retire(&restarted, &paper, "third", start + 10 * minute);
    reap_after(&restarted, &evidence, 24 * HOUR, start + 29 * minute, true);
    assert_eq!(digests().len(), 1, "nothing more within the half hour");

    reap_after(&restarted, &evidence, 24 * HOUR, start + 31 * minute, true);
    let sent = digests();
    assert!(sent.len() == 2 && sent[1].contains("2 sessions") && sent[1].contains("second: retired") && sent[1].contains("third: retired"), "{sent:?}");

    reap_after(&restarted, &evidence, 24 * HOUR, start + 62 * minute, true);
    assert_eq!(digests().len(), 2, "nothing is sent twice");
}

#[test]
fn a_session_retires_when_finished_and_its_cli_runs_nothing_when_its_host_ended_it_at_idle_exit_or_when_quiet_past_the_stale_window_unless_a_wait_gate_lane_job_or_unfinished_task_would_wake_it() {
    {
        let home = tempfile::tempdir().unwrap();
        let (fleet, paper) = fleet(home.path());
        for name in ["quiet", "waiting", "gated", "queued", "running"] {
            reported(&fleet, name, None, &format!("{name}: still on it"));
        }
        fleet.waits().register("waiting", Run::Branch("n636-reaper".into()), Path::new("/tmp"), None).unwrap();
        let gate = GateRequest { verification: false, issue: None, box_text: None, session: "gated".into(), question: "Which?".into(), options: vec!["a".into(), "b".into()], recommend: 1, default: 1, default_after_ms: 0, ask_once: None };
        paper.open_gate(&gate, now_ms()).unwrap();
        let queue = home.path().join(".local/state/north/land-queue/560e3f31dbaa2415");
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::write(queue.join("1-queued.json"), json!({"root": "/elsewhere", "branch": "queued-lane", "session": "queued"}).to_string()).unwrap();
        let evidence = Sight { running: [("running".to_owned(), Work::Detached)].into(), ..Sight::default() };
    
        let spent = reap_within(&fleet, &evidence, Duration::ZERO, now_ms(), true, Duration::ZERO, &|| Duration::ZERO);
        let fresh = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
        let stale = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);
    
        assert!(retired(&spent).is_empty() && spent.deferred == 5, "a reaper out of budget leaves every session to the next tick: {spent:?}");
        assert!(retired(&fresh).is_empty(), "{fresh:?}");
        assert_eq!(retired(&stale), ["quiet"]);
        let why = |name: &str| stale.kept.iter().find(|kept| kept.name == name).map(|kept| kept.why.clone()).unwrap_or_default();
        assert!(why("waiting").contains("branch:n636-reaper") && why("gated").contains("gate") && why("queued").contains("queued-lane") && why("running").contains("background job"), "{stale:?}");
        let note: Value = serde_json::from_slice(&std::fs::read(archived_dir(&fleet, "quiet").join("retired.json")).unwrap()).unwrap();
        assert_eq!(note["last"], "quiet: still on it");
        assert!(note["why"].as_str().unwrap().contains("quiet over 0h"), "{note}");
    }
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    reported(&fleet, "finished", Some("lead"), "Done: the fix landed as 1af7168df on origin/main.");
    reported(&fleet, "monitoring", Some("lead"), "Not done: CI is still running; its monitor wakes me.");
    reported(&fleet, "queued", Some("lead"), "Not done: the fix is written but clippy is queued in capacity admission, and the run will notify me when it ends.\n\nStop: capacity");
    reported(&fleet, "ended", Some("lead"), "Done: the arm result landed as 2bc8279e0.");
    for name in ["finished", "monitoring", "queued", "ended"] {
        paper.claim_receipt(name, "turn:1", now_ms()).unwrap();
    }
    let dir = fleet.root.join("ended");
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
    legacy["stopped"] = json!(true);
    legacy["ended"] = json!("idle-exit");
    std::fs::write(dir.join("session.json"), legacy.to_string()).unwrap();
    fleet.waits().register("ended", Run::Process { pid: 1, started: 1 }, Path::new("/tmp"), Some("sh -c d=$0 u=$1 limit=$2")).unwrap();
    let running = ["finished", "monitoring", "queued"].map(|name| (name.to_owned(), Work::Cli)).into();
    let evidence = Sight { landed: ["1af7168df".to_owned()].into(), running, ..Sight::default() };

    let fresh = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), false);
    assert_eq!(retired(&fresh), ["ended"], "a Done whose own build still runs keeps its session inside the stale window: {fresh:?}");

    let reaped = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);

    let mut names = retired(&reaped);
    names.sort_unstable();
    assert_eq!(names, ["ended", "finished"], "{reaped:?}");
    assert!(reaped.kept.iter().any(|kept| kept.name == "monitoring" && kept.why.contains("background task")), "{reaped:?}");
    assert!(reaped.kept.iter().any(|kept| kept.name == "queued" && kept.why.contains("background task")), "a Not done whose CLI still runs the job it waits on is paused, not finished (north#770): {reaped:?}");
    assert!(fleet.session("finished").is_err() && fleet.session("ended").is_err() && fleet.session("monitoring").is_ok());
}

// KEEP(bug): on 2026-11-10 at 3:20 PM 27 DeepSeek sessions whose lanes had landed and whose Done reports sat in the proxy's held digest kept their claims on seven issues until forgotten by hand; north#1054
#[test]
fn a_done_session_whose_lane_landed_gives_up_every_claim_and_leaves_the_fleet_in_one_reaper_tick() {
    use crate::fleet::claims::{Claims, Held, Rows};
    let (home, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (paper, claims) = (Arc::new(Paper::default()), Arc::new(Held::default()));
    let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), paper.clone(), claims.clone()).unwrap().arming(|_| Ok(()));
    let _lead = live(&fleet, "lead");
    let name = "ds-1041-x";
    reported(&fleet, name, Some("lead"), "Done: the box landed as 43191ff09a on origin/main.");
    paper.claim_receipt(name, "held:1", now_ms()).unwrap();
    claims.hold("north#1041@the box it worked", name, &BTreeSet::new()).unwrap();
    claims.hold("north#1041@a box it claimed itself", name, &BTreeSet::new()).unwrap();
    let stream = fleet.root.join(name).join("g1/stream.jsonl");
    std::fs::File::options().write(true).open(stream).unwrap().set_modified(std::time::UNIX_EPOCH + Duration::from_millis(now_ms() - 600_000)).unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    std::fs::create_dir_all(&queue).unwrap();
    let ticket = queue.join(format!("{}-n1041.json", (now_ms() - 12 * 60_000) * 1000));
    std::fs::write(&ticket, json!({"session": name, "branch": "n1041", "queued_micros": (now_ms() - 12 * 60_000) * 1000}).to_string()).unwrap();
    north_base::land_ticket::Records::of(&ticket).unwrap().moved(&ticket, north_base::land_ticket::Phase::Landed, "landed").unwrap();
    let evidence = Sight { landed: BTreeSet::from(["43191ff09a".to_owned()]), lanes: north_base::land_ticket::lanes_by_session(&state.path().join("north/land-queue")).unwrap(), ..Sight::default() };

    let reaped = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);

    assert_eq!(retired(&reaped), [name], "{reaped:?}");
    assert!(fleet.session(name).is_err(), "a retired session no longer stands parked beside the issue's next box");
    assert_eq!(claims.holders(Rows::Owners(&BTreeSet::from([name.to_owned()]))).unwrap(), Vec::<(String, String)>::new());
}
