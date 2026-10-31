use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::alerts::{Node, State, alert_plan};
use super::autoland::{autoland_samples, lane_counts, landed_times, percentiles};
use super::guards::{Build, actions_verdict, cargo_pins, pins_verdict, reuse_verdict, wisp_verdict};
use super::measure::project_targets;
use super::render::format_row;
use super::stall::{Facts, Medians, verdict};
use super::tests::at;
use super::throughput::{Closed, Commit, DeadLane, Inputs, throughput};

fn tip(state: &str, description: &str) -> Value {
    json!({"oid": "x", "status": {"contexts": [{"context": "autoland", "state": state, "description": description, "createdAt": "2026-10-12T03:00:00Z"}]}})
}

#[test]
fn test_landings_count_lanes_landed_not_main_commits() {
    let landed = format!("landed as {}", "a".repeat(40));
    let tips: BTreeMap<String, Value> = [
        ("tip0", tip("SUCCESS", &landed)),
        ("tip1", tip("SUCCESS", &landed)),
        ("tip2", tip("PENDING", "waiting to test alone")),
        ("tip3", tip("SUCCESS", "superseded by a later push")),
        ("tip4", Value::Null),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    assert_eq!(landed_times(&tips), vec![at("2026-10-12T03:00:00Z"); 2]);
    recorded_autoland_latency();
}

#[test]
fn test_failed_autoland_lanes_are_dead_not_pending() {
    let now = at("2026-10-12T02:26:00Z");
    let states: BTreeMap<String, Value> = [
        ("a", json!({"state": "FAILURE", "createdAt": "2026-10-11T12:26:00Z"})),
        ("b", json!({"state": "ERROR", "createdAt": "2026-10-12T01:56:00Z"})),
        ("c", json!({"state": "PENDING", "createdAt": "2026-10-12T02:20:00Z"})),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    let lanes = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
    assert_eq!(lane_counts(&lanes(&["a", "b", "c", "d"]), &states, now), (2, 2, Some(840.0)));
    assert_eq!(lane_counts(&lanes(&["c", "d"]), &states, now), (2, 0, None));
    let row = json!({"repo": "smashcraft", "pending": 2, "dead": 2, "dead_oldest_minutes": 840.0, "landed_per_hour": 1, "push_to_land_minutes": 12, "push_to_land_p90_minutes": 30});
    assert!(format_row(&row, None).contains("smashcraft: pending 2, dead 2 (oldest 840m), landed 1/h"), "{}", format_row(&row, None));
}

fn lan_lines() -> Vec<Value> {
    vec![
        json!({"pair": 0, "profile": "checks", "fps": 60, "driver": {"pid": 2400100, "since": "2026-10-11T16:10:00Z", "goneAt": "2026-10-11T16:40:00Z"}}),
        json!({"pair": 1, "profile": "checks", "fps": 60, "driver": {"pid": 2400200, "since": "2026-10-11T16:55:00Z"}}),
    ]
}

fn with(line: &Value, extra: Value) -> Value {
    let mut merged = line.clone();
    merged.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    merged
}

struct Capacity<'a> {
    pending: u64,
    samples: &'a [(f64, u64)],
    pairs: &'a [Value],
    flags: &'a [Option<bool>],
    builds: &'a [Build],
}

fn capacity_row(now: f64, case: &Capacity) -> Value {
    let stall = verdict(&Facts { now, main_at: now, landings: &[now], pending: case.pending, jobs: &[], medians: &Medians::new(), pending_since: None });
    json!({
        "repo": "smashcraft", "main_sha": "tip", "pending": case.pending, "stall": stall,
        "actions": actions_verdict(Some(case.pending), case.samples, now),
        "wisp": wisp_verdict(Some(case.pairs), Some(1), now),
        "reuse": reuse_verdict(Some(case.flags)),
        "pins": pins_verdict(Some(case.builds)),
    })
}

fn pushed_reuse(now: f64) -> Vec<Option<bool>> {
    let pushes: Vec<Value> = (0..20).map(|i| json!({"headBranch": format!("claude/land-{i}"), "headSha": format!("{i:040x}"), "createdAt": "2026-10-11T15:00:00Z"})).collect();
    let tips: BTreeMap<String, Value> = (0..20)
        .map(|i| {
            let context = json!({"context": "autoland", "state": "SUCCESS", "createdAt": format!("2026-10-11T16:{i:02}:00Z"), "description": format!("landed as {i:040x} census-reused={}", if i < 9 { "yes" } else { "no" })});
            (format!("tip{i}"), json!({"oid": format!("{i:040x}"), "status": {"contexts": [context]}}))
        })
        .collect();
    autoland_samples(&pushes, &tips, now).1
}

#[test]
fn test_recorded_11_oct_idle_capacity() {
    let now = at("2026-10-11T17:00:00Z");
    let lines = lan_lines();
    let busy_pool: Vec<Value> = lines.iter().map(|line| with(line, json!({"state": "busy", "idleSeconds": 0}))).collect();
    let reuse = pushed_reuse(now);
    let quiet: Vec<(f64, u64)> = (0..5).map(|i| (now - 900.0 - 60.0 + 300.0 * f64::from(i), 1)).collect();
    let manifest = "wc3-controller-model = { git = \"https://github.com/tompassarelli/wc3-controller\", tag = \"v0.3.3\" }\n".to_owned();
    let pins = cargo_pins(&["[package]\nname = \"smashcraft\"\n".to_owned(), manifest]);
    assert_eq!(pins, [("tompassarelli/wc3-controller".to_owned(), "v0.3.3".to_owned())]);
    let build = |conclusion: &str| Build { dependency: pins[0].0.clone(), tag: pins[0].1.clone(), status: "completed".into(), conclusion: conclusion.into() };
    let nodes = vec![Node { id: "smashcraft-parent".into(), parent: None, active: true, cwd: "/home/tom/code/smashcraft/main".into() }];
    let red = [build("failure")];
    let flagged = capacity_row(now, &Capacity { pending: 17, samples: &quiet, pairs: &lines, flags: &reuse, builds: &red });
    let (_, actions) = alert_plan(std::slice::from_ref(&flagged), &State::new(), now, &nodes);
    let kinds: BTreeSet<&str> = actions.iter().map(|action| action.key.split(':').nth(1).unwrap()).collect();
    assert_eq!(kinds, BTreeSet::from(["actions", "wisp", "reuse", "pins"]));
    assert_eq!(actions.len(), 8);
    assert!(actions.iter().find(|action| action.key.contains(":pins:")).unwrap().text.contains("tompassarelli/wc3-controller@v0.3.3 built failure"));
    let busy: Vec<(f64, u64)> = quiet.iter().map(|(at, _)| (*at, 30)).collect();
    let green = [build("success")];
    let mixed: Vec<Option<bool>> = (0..20).map(|i| Some(i < 15)).collect();
    let healthy = capacity_row(now, &Capacity { pending: 17, samples: &busy, pairs: &busy_pool, flags: &mixed, builds: &green });
    assert_eq!(alert_plan(&[healthy], &State::new(), now, &nodes), (State::new(), vec![]));
    let dead_pool = vec![with(&lines[0], json!({"state": "dead", "idleSeconds": 0, "down": ["lan0b"]})), busy_pool[1].clone()];
    let dead = wisp_verdict(Some(&dead_pool), Some(1), now);
    assert!(dead.text.starts_with("wisp 0/2 pairs idle"), "{}", dead.text);
    let reason = dead.reason.unwrap();
    assert!(reason.contains("lan0b"));
    assert!(!reason.split_once(':').unwrap().1.contains("idle"), "{reason}");
}

fn closure(number: u64, at: f64, comments: &[String]) -> Closed {
    Closed { number, closed_at: at, comments: comments.to_vec() }
}

#[test]
fn test_recorded_11_12_oct_landings_without_closures() {
    let now = at("2026-10-12T03:45:00Z");
    let landings: Vec<f64> = (1..=70).map(|i| now - 86_400.0 + 1234.0 * f64::from(i)).collect();
    let pushes: Vec<f64> = landings.iter().map(|at| at - 720.0).collect();
    let closers = [3usize, 12, 21, 30, 39, 48, 57, 64, 69];
    let shas: Vec<String> = (0..70u64).map(|i| format!("{:0<40x}", 0x1000_0000_0000 + i * 7919)).collect();
    let main_log: Vec<Commit> = landings
        .iter()
        .enumerate()
        .map(|(i, at)| Commit { sha: shas[i].clone(), at: *at, message: format!("Land lane {i}{}", if closers.contains(&i) && i % 2 == 0 { format!("\n\nFixes #{}", 400 + i) } else { String::new() }) })
        .collect();
    let closed: Vec<Closed> = closers
        .iter()
        .map(|&i| closure(400 + i as u64, landings[i] + 600.0, &if i % 2 == 1 { vec![format!("Closed by auto-close after {} landed", &shas[i][..12])] } else { vec![] }))
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("orchestration.toml");
    std::fs::write(&config, "[projects.smashcraft]\ntarget = \"2026-10-13T03:45:00Z\"\n[projects.north]\ntarget = \"2026-10-12T15:45:00Z\"\n[projects.wisp]\n").unwrap();
    let targets = project_targets(&config);
    assert_eq!(targets.keys().map(String::as_str).collect::<Vec<_>>(), ["north", "smashcraft"]);
    let claims = BTreeMap::new();
    let inputs = |repo, open, closed, log, landings, pushes, pending_since| Inputs { repo, now, open_count: Some(open), closed, main_log: log, landings, pushes, target: targets.get(repo).copied(), dead_lanes: &[], claims: &claims, pending_since };
    let waiting = [now - 900.0];
    let row = throughput(&inputs("smashcraft", 78, &closed, &main_log, &landings, &pushes, &waiting));
    assert!((row.closed_per_hour.unwrap() - 1.0 / 3.0).abs() < 1e-9);
    assert!((row.needed_per_hour.unwrap() - 78.0 / 24.0).abs() < 1e-9);
    assert_eq!(throughput(&inputs("north", 24, &[], &[], &[], &[], &[])).needed_per_hour, Some(2.0));
    assert_eq!((row.land_to_close_minutes, row.land_ratio_3h), (Some(10.0), Some(1.0)));
    assert_eq!((row.failed_unowned, row.oldest_lane_wait_minutes), (0, Some(15.0)));
    let mut formatted = json!({"repo": "smashcraft", "pending": 1, "landed_per_hour": 3, "push_to_land_minutes": 12, "push_to_land_p90_minutes": 20});
    formatted.as_object_mut().unwrap().extend(serde_json::to_value(&row).unwrap().as_object().unwrap().clone());
    formatted["goal_finish"] = json!(row.target.unwrap() + 3600.0);
    let services = [json!({"id": "build", "kind": "cloud", "usd_per_week": 12.5}), json!({"id": "host", "kind": "cloud", "billing": "fixed", "usd_per_week": 7})];
    let line = format_row(&formatted, Some(&services));
    let decisions: Vec<&str> = line.lines().filter(|part| part.starts_with("Blocked: decision for Tom:")).collect();
    assert_eq!(decisions.len(), 1, "{line}");
    assert!(decisions[0].contains("more budget (rent build/test compute: build current ledger cost $12.5/week, speedup unmeasured"));
    assert!(decisions[0].contains("reconfigure/swap service: host current ledger cost $7/week, speedup unmeasured) / later date / smaller scope"));
    for other in [json!({"goal_finish": row.target.unwrap() - 60.0}), json!({"goal_finish": null}), json!({"target": null})] {
        assert!(!format_row(&with(&formatted, other), Some(&services)).contains("Blocked: decision for Tom:"));
    }
    assert!(line.contains("closed 0.3/h needed 3.2/h, land-to-close 10m, land ratio 1.00 over 3h, failed unowned 0, oldest lane wait 15m"), "{line}");
    let failed = now - 3600.0;
    let lanes: Vec<DeadLane> = [(failed, 513), (failed, 514), (now - 300.0, 515)].into_iter().map(|(failed_at, number)| DeadLane { failed_at, issues: BTreeSet::from([number]) }).collect();
    let claimed = BTreeMap::from([(514, failed + 60.0)]);
    let stuck = throughput(&Inputs { repo: "smashcraft", now, open_count: Some(78), closed: &closed, main_log: &main_log, landings: &landings, pushes: &pushes, target: None, dead_lanes: &lanes, claims: &claimed, pending_since: &[] });
    assert_eq!((stuck.needed_per_hour, stuck.failed_unowned), (None, 1));
}

fn recorded_autoland_latency() {
    let push = |branch: &str, sha: &str, created: &str| json!({"headBranch": branch, "headSha": sha, "createdAt": created});
    let sha = |digit: char| digit.to_string().repeat(40);
    let mut pushes = vec![
        push("claude/land-recorded-1", &sha('1'), "2026-10-11T12:39:54Z"),
        push("claude/land-recorded-2", &sha('2'), "2026-10-11T12:37:05Z"),
        push("claude/land-recorded-3", &sha('3'), "2026-10-11T12:34:26Z"),
        push("claude/land-recorded-4", &sha('4'), "2026-10-11T12:27:43Z"),
        push("claude/land-recorded-5", &sha('5'), "2026-10-11T14:13:01Z"),
    ];
    pushes.extend([push("claude/land-recorded-1", &sha('1'), "2026-10-11T12:40:00Z"), push("claude/land-recorded-1", &sha('1'), "2026-10-09T12:39:54Z"), push("main", &sha('1'), "2026-10-11T12:00:00Z")]);
    let status = |digit: char, state: &str, description: String, created: &str| {
        json!({"oid": sha(digit), "status": {"contexts": [{"context": "autoland", "state": state, "description": description, "createdAt": created}]}})
    };
    let tips: BTreeMap<String, Value> = [
        ("tip0", status('1', "SUCCESS", format!("landed as {}", sha('6')), "2026-10-11T13:51:47Z")),
        ("tip1", status('2', "SUCCESS", format!("landed as {}", sha('7')), "2026-10-11T13:35:25Z")),
        ("tip2", status('3', "SUCCESS", format!("landed as {}", sha('7')), "2026-10-11T13:35:23Z")),
        ("tip3", status('4', "SUCCESS", format!("landed as {}", sha('8')), "2026-10-11T13:17:56Z")),
        ("tip4", status('5', "PENDING", "waiting to test alone".into(), "2026-10-11T14:13:12Z")),
        ("missing", Value::Null),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    let (samples, _) = autoland_samples(&pushes, &tips, at("2026-10-11T14:20:00Z"));
    assert_eq!(samples, [4313.0 / 60.0, 3500.0 / 60.0, 3657.0 / 60.0, 3013.0 / 60.0]);
    let (median, p90) = percentiles(&samples);
    assert_eq!((median, p90), (Some((3500.0 + 3657.0) / 120.0), Some(4313.0 / 60.0)));
    let row = json!({"repo": "smashcraft", "pending": 23, "landed_per_hour": 8, "push_to_land_minutes": median, "push_to_land_p90_minutes": p90});
    assert!(format_row(&row, None).contains("push-to-land median 59.6417m p90 71.8833m over 24h"), "{}", format_row(&row, None));
}

#[test]
fn test_projected_miss_compares_the_goal_eta_with_the_target() {
    let directory = tempfile::tempdir().unwrap();
    let places = super::tests::places(directory.path());
    let db = rusqlite::Connection::open(&places.ledger).unwrap();
    db.execute("CREATE TABLE calibration(at TEXT, kind TEXT, cell TEXT, n INT, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT)", []).unwrap();
    let targets = ["2026-10-12T15:59:00Z", "2026-10-12T07:00:00Z"];
    for target in targets {
        let source = json!({"predicted_finish": "2026-10-12T07:51:00Z", "boxes": 7, "width": 22, "target": target}).to_string();
        db.execute("INSERT INTO calibration VALUES('2026-10-12T07:30:00Z', 'goal_eta', 'north', 7, NULL, NULL, NULL, 21, ?1)", [source]).unwrap();
    }
    let base = json!({"repo": "north", "pending": 0, "landed_per_hour": 6, "push_to_land_minutes": 0, "push_to_land_p90_minutes": 0, "closed_per_hour": 0.0, "needed_per_hour": 14.0 / 10.6, "land_to_close_minutes": null, "land_ratio_3h": null, "failed_unowned": 0, "oldest_lane_wait_minutes": null});
    let line = |target: &str| {
        let epoch = at(target);
        let finish = super::goal::goal_finish(&places, "north", Some(epoch));
        format_row(&with(&base, json!({"target": epoch, "goal_finish": finish})), Some(&[]))
    };
    assert!(!line(targets[0]).contains("Blocked: decision for Tom:"));
    assert!(line(targets[1]).contains("Blocked: decision for Tom: north projected miss 2026-10-12T07:00:00Z"), "{}", line(targets[1]));
}
