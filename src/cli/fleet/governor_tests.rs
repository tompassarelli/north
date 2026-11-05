use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::*;

const NOW: f64 = 1_793_847_600.0;
const BOUNDS: Bounds = Bounds { drain_minutes: 30.0, launch_to_queued_minutes: 66.0, lanes_per_session: 1.18, pressure_basis_points: 2500.0 };
const NORTH: &str = "https://github.com/tompassarelli/north.git";

fn decided(done: &Path, name: &str, verdict: &str, ago_minutes: f64) {
    let path = done.join(format!("{name}.json"));
    fs::create_dir_all(done).unwrap();
    fs::write(&path, json!({"branch": name, "queued_at": "2026-11-05T10:00:00", "verdict": verdict}).to_string()).unwrap();
    OpenOptions::new().write(true).open(&path).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs_f64(NOW - ago_minutes * 60.0)).unwrap();
}

fn queued_at(ago_minutes: f64) -> String {
    let at = time::OffsetDateTime::from_unix_timestamp((NOW - ago_minutes * 60.0) as i64).unwrap();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second())
}

fn ticket(queue: &Path, micros: u128, index: usize, ago_minutes: f64) {
    fs::create_dir_all(queue).unwrap();
    fs::write(queue.join(format!("{micros}-lane-{index}.json")), json!({"branch": format!("lane-{index}"), "queued_at": queued_at(ago_minutes)}).to_string()).unwrap();
}

fn waiting(queue: &Path, index: usize, ago_minutes: f64) {
    ticket(queue, ((NOW - ago_minutes * 60.0) * 1e6) as u128, index, ago_minutes);
}

struct Queues {
    root: tempfile::TempDir,
}

impl Queues {
    fn new() -> Self {
        Self { root: tempfile::tempdir().unwrap() }
    }

    fn repo(&self, key: &str, queued: usize, landed_last_hour: usize) -> std::path::PathBuf {
        let queue = self.root.path().join(key);
        fs::create_dir_all(&queue).unwrap();
        for index in 0..landed_last_hour {
            decided(&queue.join("done"), &format!("landed-{index}"), "landed abc", index as f64 * 59.0 / landed_last_hour as f64);
        }
        for index in 0..queued {
            waiting(&queue, index, 5.0);
        }
        queue
    }
}

fn assessed(queue: &Path, load: Load) -> Assessment {
    assess("north", read_lander(queue, NOW), read_landings(&all_queues(queue), NOW), load, BOUNDS)
}

fn peer(name: &str, status: Status, brief: &str, origin: &str) -> Peer {
    Peer { name: name.into(), status, brief: brief.into(), origin: Some(origin.into()) }
}

#[test]
fn a_45_lane_queue_at_50_landings_an_hour_refuses_the_next_launch_with_the_reason() {
    let queues = Queues::new();
    let assessment = assessed(&queues.repo("north", 45, 50), Load::default());
    assert_eq!((assessment.lander.queued, assessment.rate_basis, assessment.landings_per_hour, assessment.queue_ceiling), (45, "repo", 50.0, 25.0));
    assert_eq!(
        assessment.refusal.as_deref(),
        Some("refuse: staffing: north has 45 lanes queued to land, at or over the 25.0 that 50.0 landings a busy hour drain in 30 minutes; wait for the queue to drain")
    );
}

#[test]
fn a_5_lane_queue_at_50_landings_an_hour_allows_the_launch() {
    let queues = Queues::new();
    assert_eq!(assessed(&queues.repo("north", 5, 50), Load::default()).refusal, None);
}

#[test]
fn a_lander_whose_oldest_ticket_waited_past_twice_the_drain_bound_refuses_at_depth_2() {
    let queues = Queues::new();
    let queue = queues.repo("north", 1, 50);
    waiting(&queue, 9, 80.0);
    let assessment = assessed(&queue, Load::default());
    assert_eq!((assessment.lander.queued, assessment.stall_minutes), (2, 67.0));
    assert_eq!(assessment.refusal.as_deref(), Some("refuse: staffing: north lander stalled, oldest 80 min; wait for the queue to drain"));
}

#[test]
fn a_home_with_no_lander_record_reads_as_not_stalled_and_refuses_nothing() {
    let queues = Queues::new();
    let queue = queues.repo("north", 2, 0);
    waiting(&queue, 9, 29_897_794.0);
    let assessment = assessed(&queue, Load::default());
    assert_eq!((assessment.lander.recorded, assessment.lander.queued), (false, 3));
    assert_eq!(assessment.refusal, None);
}

#[test]
fn a_jump_the_queue_ticket_with_a_low_id_queued_minutes_ago_does_not_trip_the_stall() {
    let queues = Queues::new();
    let queue = queues.repo("north", 1, 50);
    ticket(&queue, 1_699_999_999_999_970, 9, 4.0);
    let assessment = assessed(&queue, Load::default());
    assert_eq!((assessment.lander.queued, assessment.lander.oldest_pending_minutes), (2, Some(5.0)));
    assert_eq!(assessment.refusal, None);
}

#[test]
fn a_lander_that_recorded_a_round_but_landed_nothing_still_refuses_a_stalled_queue() {
    let queues = Queues::new();
    let queue = queues.repo("north", 0, 0);
    waiting(&queue, 9, 80.0);
    fs::create_dir_all(queue.join("watchdog")).unwrap();
    fs::write(queue.join(watchdog::ROUNDS), "[]").unwrap();
    let assessment = assessed(&queue, Load::default());
    assert!(assessment.lander.recorded);
    assert_eq!(assessment.refusal.as_deref(), Some("refuse: staffing: north lander stalled, oldest 80 min; wait for the queue to drain"));
}

#[test]
fn a_repository_is_named_by_its_checkout_not_its_origin_remote() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("code/makgora/main");
    let lane = scratch.path().join("code/makgora/worktrees/box");
    let git = |directory: &Path, args: &[&str]| assert!(Command::new("git").arg("-C").arg(directory).args(args).output().unwrap().status.success(), "{args:?}");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["worktree", "add", "-q", "-b", "box", lane.to_str().unwrap()]);
    assert_eq!(checkout(&main).as_deref(), Some("makgora"));
    assert_eq!(checkout(&lane).as_deref(), Some("makgora"));
    assert_eq!(checkout(scratch.path()), None);
}

#[test]
fn a_repository_with_under_10_landings_a_day_uses_the_all_repos_rate() {
    let queues = Queues::new();
    let quiet = queues.repo("north", 4, 3);
    queues.repo("wisp", 0, 40);
    let assessment = assessed(&quiet, Load::default());
    assert_eq!((assessment.rate_basis, assessment.landings_per_hour, assessment.all_repos.landed_last_day), ("all-repos", 43.0, 43));
    assert_eq!(assessment.refusal, None);
    let alone = Queues::new();
    assert!(assessed(&alone.repo("north", 4, 3), Load::default()).refusal.unwrap().contains("4 lanes queued to land, at or over the 2.0 that 3.0 landings"));
}

#[test]
fn the_rate_is_landings_per_busy_hour_or_the_last_hour_whichever_is_larger() {
    let queues = Queues::new();
    let queue = queues.repo("north", 0, 2);
    for index in 0..60 {
        decided(&queue.join("done"), &format!("earlier-{index}"), "landed abc", 180.0 + f64::from(index % 3) * 60.0 + f64::from(index) * 0.5);
    }
    decided(&queue.join("done"), "refused", "failed /logs/x.log", 200.0);
    decided(&queue.join("done"), "old", "landed abc", 1500.0);
    let landings = read_lander(&queue, NOW).landings;
    assert_eq!(landings, Landings { landed_last_hour: 2, landed_last_day: 62, busy_hours: 4 });
    assert_eq!(landings.per_hour(), 15.5);
}

#[test]
fn the_in_flight_check_runs_in_shadow_and_its_line_is_written() {
    let queues = Queues::new();
    let assessment = assessed(&queues.repo("north", 0, 50), Load { working: 66, pending_admits: 4, pressure_basis_points: None });
    assert_eq!(assessment.refusal, None);
    assert_eq!(assessment.would_refuse.as_deref(), Some("refuse: staffing: north would carry 83.8 lanes in flight with this one over the ceiling of 80.0"));
    let state = tempfile::tempdir().unwrap();
    let log = state.path().join("north/fleet-governor/decisions.jsonl");
    record(&log, &Decision { at: NOW, repo: "north", session: "worker-a", decision: "allow", note: None, staffing_ok: None, assessment: Some(&assessment) }).unwrap();
    let line: Value = serde_json::from_str(fs::read_to_string(&log).unwrap().trim()).unwrap();
    assert_eq!((line["decision"].as_str(), line["refusal"].as_str(), line["working"].as_u64(), line["pending_admits"].as_u64()), (Some("allow"), None, Some(66), Some(4)));
    assert_eq!((line["in_flight_ceiling"].as_f64(), line["landings_per_hour"].as_f64(), line["bounds"]["drain_minutes"].as_f64()), (Some(80.0), Some(50.0), Some(30.0)));
    assert!(line["would_refuse"].as_str().unwrap().contains("over the ceiling of 80.0"));
}

#[test]
fn with_nothing_landed_in_a_day_the_shadow_ceiling_is_four_lanes() {
    let queues = Queues::new();
    let queue = queues.repo("north", 0, 0);
    let two = assessed(&queue, Load { working: 2, ..Load::default() });
    assert_eq!((two.cold, two.in_flight_ceiling, two.would_refuse.as_deref()), (true, 4.0, None));
    assert!(assessed(&queue, Load { working: 3, ..Load::default() }).would_refuse.is_some());
}

#[test]
fn host_pressure_is_shadowed_not_enforced() {
    let queues = Queues::new();
    let assessment = assessed(&queues.repo("north", 5, 50), Load { pressure_basis_points: Some(2500.0), ..Load::default() });
    assert_eq!(assessment.refusal, None);
    assert_eq!(assessment.would_refuse.as_deref(), Some("refuse: staffing: the chosen host's protected CPU pressure is 25%, at or over the 25% capacity limit"));
}

#[test]
fn only_working_sessions_on_the_repository_that_land_are_counted() {
    let mut peers: Vec<Peer> = (0..3).map(|index| peer(&format!("worker-{index}"), Status::Working, "Item: north#1\nCategory: tooling\n", NORTH)).collect();
    peers.push(peer("idle", Status::Idle, "Category: tooling\n", NORTH));
    peers.push(peer("parked", Status::Parked, "Category: tooling\n", NORTH));
    peers.push(peer("starting", Status::Starting, "Category: tooling\n", NORTH));
    peers.push(peer("gone", Status::Exited, "Category: tooling\n", NORTH));
    peers.push(peer("wisp-worker", Status::Working, "Category: tooling\n", "https://github.com/tompassarelli/wisp.git"));
    peers.push(peer("loops-lead", Status::Working, "", NORTH));
    peers.push(peer("scout", Status::Working, "Category: research\n", NORTH));
    assert_eq!(working(&peers, NORTH), 3);
}

#[test]
fn admits_in_the_last_three_minutes_that_are_not_running_yet_count_as_pending() {
    let line = |at: f64, session: &str, decision: &str, repo: &str| json!({"at": at, "session": session, "decision": decision, "repo": repo}).to_string();
    let log = [
        line(NOW - 30.0, "fresh", "allow", "north"),
        line(NOW - 40.0, "fresh", "allow", "north"),
        line(NOW - 60.0, "excused", "staffing-ok", "north"),
        line(NOW - 90.0, "running", "allow", "north"),
        line(NOW - 100.0, "refused", "refuse", "north"),
        line(NOW - 120.0, "elsewhere", "allow", "wisp"),
        line(NOW - 400.0, "stale", "allow", "north"),
        "{torn".to_owned(),
    ]
    .join("\n");
    assert_eq!(pending_admits(&log, "north", &BTreeSet::from(["running".to_owned()]), NOW), 2);
}

#[test]
fn an_exempt_lander_launch_passes_and_leads_research_and_red_fixes_are_exempt() {
    let never = || panic!("only a brief naming main is red asks GitHub");
    assert_eq!(exemption("lander-repair", "Item: north#7\nCategory: tooling\n", never), Some("a lander-* launch repairs the landing path"));
    assert_eq!(exemption("loops-lead", "Category: tooling\n", never), Some("a lead or research launch produces no lane"));
    assert_eq!(exemption("lead-fleet", "", never), Some("a lead or research launch produces no lane"));
    assert_eq!(exemption("coordinator", "Delegation: role=lead budget=0\n", never), Some("a lead or research launch produces no lane"));
    assert_eq!(exemption("scout", "Item: north#9\nCategory: Research\n", never), Some("a lead or research launch produces no lane"));
    assert_eq!(exemption("worker-a1", "Item: north#112\nCategory: tooling\n", never), None);
    assert_eq!(exemption("fixer", "Item: north#600 main is red\n", || true), Some("its task is the open main is red issue"));
    assert_eq!(exemption("fixer", "Item: north#601 Main is red again?\n", || false), None);
}

#[test]
fn every_decision_appends_one_json_line() {
    let state = tempfile::tempdir().unwrap();
    let log = state.path().join("north/fleet-governor/decisions.jsonl");
    let queues = Queues::new();
    let full = assessed(&queues.repo("north", 45, 50), Load::default());
    record(&log, &Decision { at: NOW, repo: "north", session: "worker-a", decision: "refuse", note: None, staffing_ok: None, assessment: Some(&full) }).unwrap();
    record(&log, &Decision { at: NOW, repo: "north", session: "lander-fix", decision: "exempt", note: Some("a lander-* launch repairs the landing path"), staffing_ok: None, assessment: None }).unwrap();
    record(&log, &Decision { at: NOW, repo: "north", session: "repair", decision: "staffing-ok", note: None, staffing_ok: Some("repairs the lander"), assessment: Some(&full) }).unwrap();
    let lines: Vec<Value> = fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!((lines[0]["decision"].as_str(), lines[0]["queued"].as_u64(), lines[0]["queue_ceiling"].as_f64()), (Some("refuse"), Some(45), Some(25.0)));
    assert_eq!((lines[1]["decision"].as_str(), lines[1]["note"].as_str(), lines[1].get("queued")), (Some("exempt"), Some("a lander-* launch repairs the landing path"), None));
    assert_eq!((lines[2]["decision"].as_str(), lines[2]["staffing_ok"].as_str()), (Some("staffing-ok"), Some("repairs the lander")));
}

#[test]
fn the_shipped_defaults_are_the_2026_11_05_measurements() {
    let defaults = default_bounds();
    assert_eq!((defaults.drain_minutes, defaults.launch_to_queued_minutes, defaults.lanes_per_session), (30.0, 66.0, 1.18));
}

#[test]
fn staffing_ok_is_recorded_beside_the_placement_note() {
    assert_eq!(noted(None, Some("repairs the lander")), Some("staffing-ok: repairs the lander".into()));
    assert_eq!(noted(Some("overlap-ok: x".into()), Some("repairs the lander")), Some("overlap-ok: x; staffing-ok: repairs the lander".into()));
    assert_eq!(noted(Some("pin: y".into()), None), Some("pin: y".into()));
}

#[test]
fn every_spelling_of_an_origin_names_its_repository_and_its_github_slug() {
    assert_eq!(name("git@github.com:tompassarelli/north.git"), "north");
    assert_eq!(name("https://github.com/tompassarelli/wisp"), "wisp");
    assert_eq!(name("/srv/git/smashcraft.git/"), "smashcraft");
    assert_eq!(slug("git@github.com:tompassarelli/north.git").as_deref(), Some("tompassarelli/north"));
    assert_eq!(slug("https://github.com/tompassarelli/wisp").as_deref(), Some("tompassarelli/wisp"));
    assert_eq!(slug("/srv/git/smashcraft.git"), None);
}

#[test]
fn the_queue_hold_refuses_a_proxy_even_with_staffing_ok_and_admits_only_a_brief_naming_the_ticket_at_the_front() {
    use north_landing::land::queue::{Ticket, file_name, write};
    let queues = Queues::new();
    let queue = queues.repo("north", 0, 50);
    let behind: Vec<(u128, String)> = (0..30).map(|index| (3_000_000 + index, format!("lane-{index}"))).collect();
    for (micros, branch) in [(1_000_000, "fix-lander".to_owned()), (2_000_000, "worker-lane".to_owned())].into_iter().chain(behind) {
        let ticket = Ticket { origin: NORTH.into(), branch: branch.clone(), queued_at: queued_at(5.0), ..Default::default() };
        write(&queue.join(file_name(micros, &branch)), &ticket).unwrap();
    }
    let front = front(&queue);
    assert_eq!(front.as_deref(), Some("1000000-fix-lander"));
    let assessment = assessed(&queue, Load::default());
    let refusal = assessment.refusal.as_deref();
    assert!(refusal.is_some());
    let worker = "Item: north#9 a feature\n";
    let fix = "Item: north#9 repair the lander\nTicket: 1000000-fix-lander\n";
    let second = "Item: north#9 a feature\nTicket: 2000000-worker-lane\n";
    let Hold::Refused(said) = hold(refusal, worker, front.as_deref(), Some("repairs the lander"), true) else { panic!("the proxy overrode the queue hold") };
    assert!(said.contains("the proxy cannot override the queue hold with --staffing-ok") && said.contains("(1000000-fix-lander)"), "{said}");
    assert!(matches!(hold(refusal, second, front.as_deref(), Some("repairs the lander"), true), Hold::Refused(_)), "a ticket behind the front does not pass");
    assert_eq!(hold(refusal, fix, front.as_deref(), Some("repairs the lander"), true), Hold::Front);
    assert_eq!(hold(refusal, fix, front.as_deref(), None, true), Hold::Front);
    assert_eq!(hold(refusal, worker, front.as_deref(), Some("repairs the lander"), false), Hold::Excused, "a session that is not the proxy keeps --staffing-ok");
    assert_eq!(hold(refusal, worker, front.as_deref(), None, false), Hold::Refused(refusal.unwrap().to_owned()));
    assert_eq!(hold(None, worker, front.as_deref(), Some("x"), true), Hold::Clear);
    assert_eq!((Hold::Front.label(), Hold::Excused.label()), ("front-of-queue", "staffing-ok"));
}
