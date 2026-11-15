use super::*;

const NOW: u64 = 1_800_000_000;

fn iso(unix: u64) -> String {
    let (days, rest) = (unix / 86_400, unix % 86_400);
    let date = time::Date::from_julian_day(i32::try_from(days).unwrap() + 2_440_588).unwrap();
    format!("{date}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[test]
fn a_possible_red_main_alert_skips_a_lane_on_a_stale_base_and_clears_once_a_later_main_commit_passes_cargo() {
    use crate::watch::facts::{RepoFacts, Run};
    let failed = |lane: &str, ago: u64| Event { time: (NOW - ago) as f64, repo: "north".into(), lane: lane.into(), stage: "land check".into(), status: "failed".into(), tests: vec!["agent_text_reads".into()], ..Event::default() };
    let cargo = |conclusion: &str, ago: u64| Run { name: "Cargo".into(), head_branch: "main".into(), event: "push".into(), status: "completed".into(), conclusion: Some(conclusion.into()), created_at: iso(NOW - ago), ..Run::default() };
    let watch = |runs: Vec<Run>| WatchContext { now: NOW, events: vec![failed("lane-a", 1500), failed("lane-b", 1200)], repos: vec![RepoFacts { name: "north".into(), runs, ..RepoFacts::default() }], ..WatchContext::default() };
    let keys = |context: &WatchContext| shared_failures(context).into_iter().map(|alert| alert.key).collect::<Vec<_>>();
    assert_eq!(keys(&watch(vec![])), ["land:north:possible-red-main:agent_text_reads"]);
    assert_eq!(keys(&watch(vec![cargo("success", 1800), cargo("failure", 600)])), ["land:north:possible-red-main:agent_text_reads"], "a pass before the failures or a red run after them does not clear it");
    assert!(keys(&watch(vec![cargo("failure", 900), cargo("success", 300)])).is_empty(), "a later main passing Cargo clears it on the next tick");
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| String::from_utf8(std::process::Command::new("git").arg("-C").arg(dir.path()).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap().stdout).unwrap().trim().to_owned();
    git(&["init", "-q", "-b", "main"]);
    let commit = |message: &str| {
        git(&["commit", "-q", "--allow-empty", "-m", message]);
        git(&["rev-parse", "HEAD"])
    };
    let old = commit("old main");
    let fix = commit("fix serve_token_scoped");
    let (on_fix, also_on_fix) = (commit("lane b"), commit("lane c"));
    git(&["checkout", "-q", "-b", "stale", &old]);
    let stale = commit("lane a");
    let event = |lane: &str, sha: &str, status: &str, ago: u64| Event { time: (NOW - ago) as f64, repo: "north".into(), lane: lane.into(), sha: sha.into(), stage: if status == "landed" { "landed".into() } else { "land check".into() }, status: status.into(), tests: vec!["serve_token_scoped".into()], ..Event::default() };
    let keys = |lanes: [(&str, &str); 2]| {
        let mut events = vec![event("fix", &fix, "landed", 2000)];
        events.extend(lanes.iter().map(|(lane, sha)| event(lane, sha, "failed", 1000)));
        crate::watch::facts::mark_stale_bases(&mut events, |_| dir.path().to_path_buf());
        shared_failures(&WatchContext { now: NOW, events, ..WatchContext::default() }).into_iter().map(|alert| alert.key).collect::<Vec<_>>()
    };
    assert!(keys([("lane-a", &stale), ("lane-b", &on_fix)]).is_empty(), "on 2026-11-14 lane 9d60524c98, based before the fix d90ce1563d, counted toward a possible red main");
    assert_eq!(keys([("lane-c", &also_on_fix), ("lane-b", &on_fix)]), ["land:north:possible-red-main:serve_token_scoped"]);
}
