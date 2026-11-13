use super::{refused, since_instant};
use crate::foundation::clock::Timestamp;
use crate::platform::records_mirror::{ROOT, Records, STAMP, Stamp};
use std::fs;

fn mirrored(state: &std::path::Path, at: f64) {
    let root = state.join(ROOT);
    let stamp = Stamp { host: "whiterabbit".into(), state: "/home/tom/.local/state".into(), at };
    fs::create_dir_all(root.join(STAMP).parent().unwrap()).unwrap();
    fs::write(root.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
}

#[test]
fn a_stale_lander_mirror_is_refused_not_passed() {
    let state = tempfile::tempdir().unwrap();
    let now = 1_794_270_600.0;
    mirrored(state.path(), now - 26.0 * 3600.0);
    let records = Records::of(state.path(), "gex45");
    let why = refused(&records, "2026-11-09T00:30:00Z", None, now).expect("a 26 h old mirror cannot judge this window");
    assert!(why.contains("stale mirror"), "{why}");
    assert!(why.contains("whiterabbit"), "{why}");
    assert!(why.contains("run it there"), "{why}");

    let fresh = tempfile::tempdir().unwrap();
    mirrored(fresh.path(), now - 30.0);
    assert_eq!(refused(&Records::of(fresh.path(), "gex45"), "2026-11-09T00:30:00Z", None, now), None, "a mirror the fleet tick just refreshed still answers");

    let local = tempfile::tempdir().unwrap();
    mirrored(local.path(), now - 26.0 * 3600.0);
    assert_eq!(refused(&Records::of(local.path(), "whiterabbit"), "2026-11-09T00:30:00Z", None, now), None, "the lander host reads its own records, however old they are");
}

#[test]
fn a_full_run_failure_names_the_lane_whose_own_run_first_failed_the_test_not_a_later_lane_landed_on_red_main() {
    use super::responsible::{Blame, responsible};
    use std::collections::{BTreeMap, BTreeSet};
    let test = "worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state";
    let chain: Vec<String> = ["5f42fb5463", "95afbfbcab", "ce30b6ea43"].map(str::to_owned).to_vec();
    let red = |sha: &str| (sha.to_owned(), BTreeSet::from([test.to_owned()]));
    let runs = BTreeMap::from([red("5f42fb5463"), red("ce30b6ea43")]);
    let mut landed = BTreeMap::from([("5f42fb5463".to_owned(), vec!["ds-1033".to_owned()]), ("95afbfbcab".to_owned(), vec!["dash".to_owned()]), ("ce30b6ea43".to_owned(), vec!["later".to_owned()])]);
    assert_eq!(responsible(&chain, &runs, &landed, test), Blame::Lane { lane: "ds-1033".into(), sha: "5f42fb5463".into() });
    landed.insert("5f42fb5463".to_owned(), vec!["ds-1033".to_owned(), "batched".to_owned()]);
    assert!(matches!(responsible(&chain, &runs, &landed, test), Blame::Unjudged(why) if why.contains("bisection")), "a batch gives no single lane without the lander's bisection");

    use super::Landing;
    use super::full_runs::{Conclusion, Run, Source};
    use crate::land::slots::graph_selection::Selected;
    let repository = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repository.path())
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "test"]);
    git(&["commit", "--allow-empty", "-qm", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    git(&["commit", "--allow-empty", "-qm", "lane"]);
    let lane = git(&["rev-parse", "HEAD"]);
    git(&["commit", "--allow-empty", "-qm", "batch"]);
    let batch = git(&["rev-parse", "HEAD"]);
    let batch_landing = Landing { base: base.clone(), head: batch, selected: Selected::default() };
    let lane_landing = Landing { base, head: lane.clone(), selected: Selected { tests: BTreeSet::from([test.to_owned()]), ..Selected::default() } };
    let run = Run { source: Source::Actions(1), sha: lane, conclusion: Conclusion::Failure, created: "2026-11-09T00:38:39Z".into() };
    let tally = super::after_landing(repository.path(), &[batch_landing, lane_landing], None, &run, None, &[test.to_owned()], &|_| false);
    assert!(tally.left_out.is_empty(), "the lane's own landing held {} though the batch's narrowed selection did not: {:?}", test, tally.left_out);
}

#[test]
fn a_relative_since_becomes_a_date_gh_can_filter_on() {
    let now = Timestamp::parse_rfc3339("2026-11-10T15:00:00Z").unwrap();
    assert_eq!(since_instant("24h", now), "2026-11-09T15:00:00Z");
    assert_eq!(since_instant("2026-11-09T01:00:00Z", now), "2026-11-09T01:00:00Z");
}

#[test]
fn the_selection_holds_the_three_tests_missed_found_left_out() {
    use super::Landing;
    use super::flaky::{Seen, before};
    use super::full_runs::{Conclusion, Run, Source};
    use crate::land::slots::graph_selection::Selected;
    use std::collections::BTreeSet;
    let gate = "fleet::registry::gate_rows::tests::a_gate_closed_while_its_answer_could_not_be_handed_in_still_reaches_the_session_when_the_tick_hands_the_spooled_row_in";
    let spool = "fleet::registry::messages::tests::a_hand_in_that_fails_is_withdrawn_from_the_spool_and_the_row_can_be_handed_in_later";
    let floor = "providers::claude::fleet::fleet_tick::a_lead_or_proxy_at_its_floor_goes_to_the_source_above_the_floor_with_most_headroom_else_parks_until_the_soonest_windfall_and_a_worker_on_spent_api_credit_waits_for_its_grant";
    let repository = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("git").arg("-C").arg(repository.path()).args(arguments).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(&["init", "-q"]);
    let commit = |message: &str| {
        git(&["-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "--allow-empty", "-qm", message]);
        git(&["rev-parse", "HEAD"])
    };
    let names = |tests: &[&str]| tests.iter().map(|test| (*test).to_owned()).collect::<BTreeSet<_>>();
    let run = |sha: &str, conclusion: Conclusion| Run { source: Source::Actions(1), sha: sha.to_owned(), conclusion, created: "2026-11-10T06:16:00Z".into() };
    let seen = |sha: &str, failed: &[&str], names_its_failures: bool| Seen { sha: sha.to_owned(), failed: names(failed), names_its_failures };
    let judge = |landings: &[Landing], seen: &[Seen], red: &str, failed: &[&str]| {
        let since = before(repository.path(), seen, red);
        let failed: Vec<String> = failed.iter().map(|test| (*test).to_owned()).collect();
        super::after_landing(repository.path(), landings, None, &run(red, Conclusion::Failure), since, &failed, &|_| false)
    };

    let green = commit("8b91c5af1f22, the last full run that passed both hand-in tests");
    let socket = commit("fb6a13237525 moves the session socket; its lane's alone pass ran both hand-in tests, so the change graph carried them");
    let socket_landing = Landing { base: green.clone(), head: socket.clone(), selected: Selected { carried: names(&[gate, spool]), ..Selected::default() } };
    let unnamed = commit("d29de10ca4b8, whose full run names no failed test");
    let glance = commit("7c636cca1d58 adds north glance");
    let glance_landing = Landing { base: unnamed.clone(), head: glance.clone(), selected: Selected::default() };
    let hand_in = [socket_landing, glance_landing];

    let at_socket = judge(&hand_in, &[seen(&green, &[], true)], &socket, &[gate, spool]);
    assert!(at_socket.left_out.is_empty() && at_socket.judged == 2, "fb6a13237525's selection carried both: {:?}", at_socket.left_out);
    let at_glance = judge(&hand_in, &[seen(&green, &[], true), seen(&socket, &[gate, spool], true), seen(&unnamed, &[], false)], &glance, &[gate, spool]);
    assert!(at_glance.left_out.is_empty() && at_glance.environment == 2, "7c636cca1d58's base already failed both, past the run that names nothing: {:?}", at_glance.left_out);

    let passed = commit("7411b2e1e021, the last full run that passed the floor test");
    let account = commit("a041871c9aaa keeps a session on an API organization out of the hand-over");
    let grant = commit("af73b62a1aa8 gives the session-scope join its own module");
    let canary = commit("060eaa5e584d probes the hub only on a host that reaches it");
    let floor_landings = [
        Landing { base: passed.clone(), head: account.clone(), selected: Selected { tests: names(&[floor]), ..Selected::default() } },
        Landing { base: account, head: grant.clone(), selected: Selected::default() },
        Landing { base: grant, head: canary.clone(), selected: Selected::default() },
    ];
    let at_canary = judge(&floor_landings, &[seen(&passed, &[], true)], &canary, &[floor]);
    assert!(at_canary.left_out.is_empty() && at_canary.judged == 1, "a041871c9aaa's selection held the floor test, though its and af73b62a1aa8's full runs were cancelled: {:?}", at_canary.left_out);
}
