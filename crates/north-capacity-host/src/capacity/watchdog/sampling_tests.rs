use serde_json::{Value, json};

use super::sampling::{Kind, State, git_maintenance_verb, step};
use super::snapshot::snapshot;

const USER: &str = "/user.slice/user-1000.slice/user@1000.service/";
const START_MS: i64 = 1_700_000_000_000;

fn process(pid: i64, ppid: i64, sid: i64, args: &[&str], cgroup: &str, rate: f64, seconds: i64) -> Value {
    json!({
        "pid": pid, "ppid": ppid, "sid": sid, "comm": args[0].rsplit('/').next().unwrap().chars().take(15).collect::<String>(), "args": args,
        "cgroup": cgroup, "ticks": (rate * 100.0 * seconds as f64) as i64, "start": (pid * 10).to_string(), "rssKiB": 1024, "cwd": "/srv/repo/main"
    })
}

fn snapshots() -> Vec<String> {
    let app = format!("{USER}app.slice/app-niri-ghostty-1.scope");
    let scope = format!("{USER}agent.slice/agent-capacity.slice/agent-capacity-{}.scope", "a".repeat(32));
    let nightly = format!("{USER}app.slice/git-maintenance-nightly.service");
    (0..6)
        .map(|i| {
            let t = i * 30;
            let mut procs = vec![process(400, 1, 400, &["claude"], &app, 0.2, t), process(500, 400, 500, &["git", "maintenance", "run", "--auto", "--detach"], &app, 0.0, t)];
            procs.extend((0..5).map(|k| process(501 + k, 500, 500, &["git", "pack-objects", "--all"], &app, 1.0, t)));
            procs.extend((0..6).map(|k| process(601 + k, 1, 600, &["cargo", "build"], &scope, 1.0, t)));
            procs.extend((0..4).map(|k| process(701 + k, 1, 700, &["bun", "test"], &app, 0.975, t)));
            procs.push(process(800, 1, 800, &["git", "-C", "/x", "repack", "-d"], &nightly, 0.5, t));
            procs.push(process(900, 400, 400, &["git", "fetch", "origin"], &app, 0.1, t));
            json!({
                "at": START_MS + t * 1000, "load": [1, 1, 1], "cpuPsi": 50, "cpuPsi60": 40, "memPsi": 0, "memFullPsi": 0,
                "memAvailableMiB": 60000, "memTotalMiB": 96000, "cgroups": {}, "procs": procs, "leases": []
            })
            .to_string()
        })
        .collect()
}

struct Replayed {
    trees: Vec<Value>,
    terminated: Vec<i64>,
}

fn replayed() -> Replayed {
    let (mut state, mut trees, mut terminated) = (State::default(), Vec::new(), Vec::new());
    for line in snapshots() {
        let stepped = step(state, serde_json::from_str(&line).unwrap());
        state = stepped.state;
        trees.extend(stepped.incidents.iter().filter(|incident| incident.kind == Kind::UnleasedTree).map(|incident| serde_json::to_value(incident).unwrap()));
        terminated.extend(stepped.terms.iter().map(|term| term.pid));
    }
    terminated.sort_unstable();
    terminated.dedup();
    Replayed { trees, terminated }
}

#[test]
fn an_unleased_five_core_tree_held_for_two_minutes_is_one_incident_and_leased_or_smaller_load_is_not() {
    let Replayed { trees, .. } = replayed();
    assert_eq!(trees.len(), 1, "{trees:#?}");
    assert_eq!(trees[0]["cgroup"], "app.slice/app-niri-ghostty-1.scope");
    assert_eq!(trees[0]["seconds"], 120);
}

#[test]
fn the_incident_records_cwd_and_the_parent_chain_up_to_the_agent() {
    let Replayed { trees, .. } = replayed();
    let chain = trees[0]["chain"].as_array().unwrap();
    assert_eq!(trees[0]["cwd"], "/srv/repo/main");
    assert_eq!(chain.len(), 3);
    assert!(chain[1].as_str().unwrap().starts_with("500 git maintenance") && chain[2].as_str().unwrap().starts_with("400 claude"));
}

#[test]
fn git_maintenance_and_its_pack_objects_outside_the_timer_are_terminated_but_the_nightly_repack_and_fetch_are_not() {
    assert_eq!(replayed().terminated, [500, 501, 502, 503, 504, 505]);
}

#[test]
fn only_gc_repack_and_maintenance_verbs_of_git_are_maintenance() {
    let verb = |words: &[&str]| git_maintenance_verb(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).map(str::to_owned);
    assert_eq!(verb(&["/usr/bin/git", "-C", "/x", "repack", "-d"]).as_deref(), Some("repack"));
    assert_eq!(verb(&["git", "--no-pager", "gc"]).as_deref(), Some("gc"));
    assert_eq!(verb(&["git", "fetch", "origin"]), None);
    assert_eq!(verb(&["cargo", "gc"]), None);
}

#[test]
fn the_live_snapshot_reads_finite_kernel_pressure_and_memory() {
    let reading = step(State::default(), snapshot(std::path::Path::new("north-not-installed"))).reading;
    let value = serde_json::to_value(&reading).unwrap();
    assert!(["cpuPsi", "cpuPsi60", "memPsi", "memFullPsi"].iter().all(|key| value[key].as_f64().is_some_and(f64::is_finite)));
    let (available, total) = (value["memAvailableMiB"].as_f64().unwrap(), value["memTotalMiB"].as_f64().unwrap());
    assert!(0.0 < available && available <= total);
}
