use north_coverage::hunks::{Hunks, parse};
use north_graph::{Action, Graph, Term, Triple};

use crate::blame::{self, Suspect};
use crate::carry::{self, Pass};
use crate::cone::{self, Added, Cone};
use crate::facts::{Facts, TestId};
use crate::lane::{Change, Changed, Status};
use crate::overlap;

fn graph(facts: &[(&str, &str, &str)]) -> Graph {
    let mut graph = Graph::open_in_memory().unwrap();
    let actions: Vec<Action> = facts.iter().map(|(subject, predicate, object)| Action::assert(Triple::new(Term::string(*subject), Term::keyword(*predicate), Term::string(*object)))).collect();
    graph.apply("test", &actions).unwrap();
    graph
}

fn id(binary: &str, name: &str) -> TestId {
    TestId { binary: binary.to_owned(), name: name.to_owned() }
}

fn changed(paths: &[&str], hunks: Hunks) -> Change {
    Change { files: paths.iter().map(|path| Changed { path: (*path).to_owned(), status: Status::Modified }).collect(), hunks, ..Change::default() }
}

const GATE_ROWS: &str = "test:north-fleet-supervisor::gate::gate_rows";
const LOOP_TICK: &str = "test:north-fleet-supervisor::tick::loop_tick";

#[test]
fn a_data_file_a_test_reads_selects_that_test_without_a_code_edge() {
    let graph = graph(&[
        (GATE_ROWS, "in-binary", "north-fleet-supervisor"),
        (LOOP_TICK, "in-binary", "north-fleet-supervisor"),
        (GATE_ROWS, "reads", "harness/defaults.toml"),
        (GATE_ROWS, "read-set", "set-1"),
        (LOOP_TICK, "read-set", "set-2"),
        ("reads:north", "reads-commit", "bde3db9c6"),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let cone = cone::of(&facts, &changed(&["harness/defaults.toml"], Hunks::new()), None).unwrap();
    assert_eq!(cone.untraced, None);
    assert_eq!(cone.tests.into_iter().collect::<Vec<_>>(), vec![id("north-fleet-supervisor", "gate::gate_rows")]);
    assert!(cone.always.is_empty());
}

#[test]
fn blame_points_each_failure_at_the_lanes_holding_it_in_their_cone_their_added_tests_or_being_untraced() {
    let graph = graph(&[
        ("test:north::it::agent_text_reads", "in-binary", "north::it"),
        ("test:north::it::fleet_relay", "in-binary", "north::it"),
        ("coverage:north", "commit", "c0ffee"),
        ("fn:src/cli/scoreboard/losses.rs:10-40", "in-file", "src/cli/scoreboard/losses.rs"),
        ("fn:crates/north-fleet/src/fleet/upward.rs:5-30", "in-file", "crates/north-fleet/src/fleet/upward.rs"),
        ("test:north::it::agent_text_reads", "reaches", "fn:src/cli/scoreboard/losses.rs:10-40"),
        ("test:north::it::fleet_relay", "reaches", "fn:crates/north-fleet/src/fleet/upward.rs:5-30"),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let hunk = |file: &str, line: u32| parse(&format!("--- a/{file}\n+++ b/{file}\n@@ -{line} +{line} @@\n"));
    let losses = cone::of(&facts, &changed(&["src/cli/scoreboard/losses.rs"], hunk("src/cli/scoreboard/losses.rs", 20)), Some(&Hunks::new())).unwrap();
    let relay = cone::of(&facts, &changed(&["crates/north-fleet/src/fleet/upward.rs"], hunk("crates/north-fleet/src/fleet/upward.rs", 12)), Some(&Hunks::new())).unwrap();
    let climb = Cone { untraced: Some("Cargo.lock does not parse".to_owned()), files: vec!["Cargo.lock".to_owned(), "crates/north-climb/src/lineage.rs".to_owned()], ..Cone::default() };
    let claims = Cone { added: vec![Added { package: "north-threads".to_owned(), name: "a_refused_claim_release_row".to_owned() }], files: vec!["crates/north-threads/src/threads/client/tests.rs".to_owned()], ..Cone::default() };
    let suspects = [Suspect { name: "n909-relay", cone: &relay }, Suspect { name: "n903-losses", cone: &losses }, Suspect { name: "n882-climb", cone: &climb }, Suspect { name: "n706-claims", cone: &claims }];
    let log = "        FAIL [   3.430s] (  51/1148) north::it agent_text_reads
  stdout ───

    running 1 test
    test agent_text_reads ... FAILED

    failures:

    failures:
        agent_text_reads

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 133 filtered out; finished in 3.42s

  stderr ───

    thread 'agent_text_reads' (24675) panicked at crates/north-climb/src/lineage.rs:87:5:
    src/cli/scoreboard/losses.rs: 1 lines read agent text, harness/agent-text-reads.toml allows 0
        PASS [   0.558s] (  52/1148) north::it land_scan::test_integrity
        FAIL [   0.210s] (  53/1148) north runtime::reuse::embeds_every_source
  stderr ───

    crates/north-climb/src/lineage.rs is not embedded
        FAIL [   0.120s] (  54/1148) north-threads threads::client::tests::a_refused_claim_release_row
  stderr ───

    thread 'threads::client::tests::a_refused_claim_release_row' panicked at crates/north-threads/src/threads/client/tests.rs:40:9:
    the release row was not sent again
docs/references.md cites crates/north-climb/src/lineage.rs
";
    let found = blame::of(&blame::from_log(log, ""), &suspects, &facts);
    assert_eq!(found.by_failure["agent_text_reads"], vec!["n903-losses".to_owned()]);
    assert_eq!(found.by_failure["threads::client::tests::a_refused_claim_release_row"], vec!["n882-climb".to_owned(), "n706-claims".to_owned()]);
    assert_eq!(found.by_failure["runtime::reuse::embeds_every_source"], vec!["n882-climb".to_owned()]);
    assert_eq!(found.lanes, vec!["n882-climb".to_owned(), "n903-losses".to_owned(), "n706-claims".to_owned()]);
    assert!(found.untraced.is_empty());
}

#[test]
fn overlap_keeps_disjoint_cones_apart_and_no_always_test_links_them() {
    let graph = graph(&[
        ("test:north-lander::a", "in-binary", "north-lander"),
        ("test:north-lander::b", "in-binary", "north-lander"),
        ("test:north-lander::unrecorded", "in-binary", "north-lander"),
        ("test:north-lander::a", "reads", "harness/a.toml"),
        ("test:north-lander::b", "reads", "harness/b.toml"),
        ("test:north-lander::a", "read-set", "set-a"),
        ("test:north-lander::b", "read-set", "set-b"),
        ("reads:north", "reads-commit", "bde3db9c6"),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let lane = |path: &str| -> Cone { cone::of(&facts, &changed(&[path], Hunks::new()), None).unwrap() };
    let lanes = vec![("a".to_owned(), lane("harness/a.toml")), ("b".to_owned(), lane("harness/b.toml"))];
    assert!(lanes.iter().all(|(_, cone)| cone.always.contains(&id("north-lander", "unrecorded"))));
    let found = overlap::of(&lanes, || facts.all_tests());
    assert_eq!(found.groups, vec![vec!["a".to_owned()], vec!["b".to_owned()]]);
    assert!(found.pairs.is_empty());
    assert_eq!(found.always.into_iter().collect::<Vec<_>>(), vec![id("north-lander", "unrecorded")]);
}

#[test]
fn the_nextest_arguments_select_each_traced_test_by_binary_and_each_added_test_by_package() {
    let cone = Cone { tests: [id("north::it", "fleet::a_launch_claims"), id("north-base", "error::tests::drill")].into(), added: vec![Added { package: "north-lanes".to_owned(), name: "retires_a_pin".to_owned() }], ..Cone::default() };
    assert_eq!(cone.nextest_arguments().join(" "), "-p north -p north-base -p north-lanes -E (binary_id(north-base)&test(=error::tests::drill))|(binary_id(north::it)&test(=fleet::a_launch_claims))|(package(north-lanes)&test(/(^|::)retires_a_pin$/))");
    assert!(Cone::default().nextest_arguments().is_empty());
}

#[test]
fn an_alone_pass_carries_only_a_test_no_other_landed_lane_and_no_landing_it_did_not_see_can_affect() {
    let cone = |tests: &[&str]| Cone { tests: tests.iter().map(|name| id("north::it", name)).collect(), ..Cone::default() };
    let lanes = vec![("lane-a".to_owned(), cone(&["alone", "shared", "unseen"])), ("lane-b".to_owned(), cone(&["shared", "reverted"])), ("revert".to_owned(), cone(&["reverted"]))];
    let passes = [Pass { lane: "lane-a".to_owned(), ran: cone(&["alone", "shared", "unseen"]), unseen: cone(&["unseen"]) }, Pass { lane: "lane-b".to_owned(), ran: cone(&["shared", "reverted"]), unseen: Cone::default() }];
    assert_eq!(carry::carried(&lanes, &passes).into_iter().collect::<Vec<_>>(), vec![id("north::it", "alone")]);
    let untraced = [lanes, vec![("toolchain".to_owned(), Cone { untraced: Some("rust-toolchain.toml".to_owned()), ..Cone::default() })]].concat();
    assert!(carry::carried(&untraced, &passes).is_empty());
}

#[test]
fn a_test_the_graph_does_not_know_runs_when_the_lane_changes_a_crate_its_package_depends_on() {
    let graph = graph(&[
        ("test:north::it::worker_sandbox::a_known_test", "in-binary", "north::it"),
        ("crate:north", "root", "."),
        ("crate:north-runtime", "root", "crates/north-runtime"),
        ("crate:north-ops", "root", "crates/north-ops"),
        ("crate:north", "depends-on", "crate:north-runtime"),
        ("coverage:north", "commit", "c0ffee"),
        ("fn:crates/north-runtime/src/worker_sandbox/mod.rs:1-9", "in-file", "crates/north-runtime/src/worker_sandbox/mod.rs"),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let file = "crates/north-runtime/src/worker_sandbox/mod.rs";
    let mut change = changed(&[file], parse(&format!("--- a/{file}\n+++ b/{file}\n@@ -4 +4 @@\n")));
    let present = |file: &str, name: &str| crate::lane::AddedTest { file: file.to_owned(), name: name.to_owned(), package: None };
    change.present = vec![
        present("tests/it/worker_sandbox.rs", "a_known_test"),
        present("tests/it/worker_sandbox.rs", "a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state"),
        present("crates/north-runtime/src/worker_sandbox/tests.rs", "a_new_unit_test"),
        present("crates/north-ops/src/report.rs", "a_test_outside_the_closure"),
    ];
    let cone = cone::of(&facts, &change, Some(&Hunks::new())).unwrap();
    assert!(cone.tests.is_empty(), "the map holds no edge from the changed region");
    let added = |package: &str, name: &str| Added { package: package.to_owned(), name: name.to_owned() };
    assert_eq!(
        cone.added,
        vec![added("north", "a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state"), added("north-runtime", "a_new_unit_test")],
        "the unknown tests of packages that link the changed crate run; known tests and packages outside the closure do not"
    );
}

#[test]
fn a_changed_constant_selects_the_tests_whose_reached_functions_use_it() {
    let graph = graph(&[
        ("test:north-base::limits::capped", "in-binary", "north-base"),
        ("test:north-base::limits::shape", "in-binary", "north-base"),
        ("test:north::it::fleet_relay", "in-binary", "north::it"),
        ("coverage:north", "commit", "c0ffee"),
        ("crate:north-base", "root", "crates/north-base"),
        ("crate:north", "root", "."),
        ("crate:north", "depends-on", "crate:north-base"),
        ("fn:crates/north-base/src/limits.rs:20-30", "in-file", "crates/north-base/src/limits.rs"),
        ("fn:crates/north-base/src/cap.rs:5-12", "in-file", "crates/north-base/src/cap.rs"),
        ("test:north-base::limits::shape", "reaches", "fn:crates/north-base/src/limits.rs:20-30"),
        ("test:north-base::limits::capped", "reaches", "fn:crates/north-base/src/cap.rs:5-12"),
        ("item:crates/north-base/src/limits.rs:3-3", "item-in", "crates/north-base/src/limits.rs"),
        ("item:crates/north-base/src/limits.rs:3-3", "defines", "sym:north-base limits/BASE."),
        ("item:crates/north-base/src/limits.rs:4-4", "item-in", "crates/north-base/src/limits.rs"),
        ("item:crates/north-base/src/limits.rs:4-4", "defines", "sym:north-base limits/LIMIT."),
        ("item:crates/north-base/src/limits.rs:4-4", "refers-to", "sym:north-base limits/BASE."),
        ("fn:crates/north-base/src/cap.rs:5-12", "refers-to", "sym:north-base limits/LIMIT."),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let limits = "crates/north-base/src/limits.rs";
    let at = |line: u32| changed(&[limits], parse(&format!("--- a/{limits}\n+++ b/{limits}\n@@ -{line} +{line} @@\n")));
    let base = at(3);
    let today = cone::of(&facts, &base, Some(&Hunks::new())).unwrap();
    assert_eq!(today.tests.into_iter().collect::<Vec<_>>(), vec![id("north-base", "limits::shape")]);
    let proof = cone::proof(&facts, &base, Some(&Hunks::new()));
    assert_eq!(proof, cone::Proof { tests: [id("north-base", "limits::capped")].into(), untraced: None });
    let unindexed = cone::proof(&facts, &at(8), Some(&Hunks::new()));
    assert_eq!(unindexed.tests, [id("north-base", "limits::capped"), id("north-base", "limits::shape"), id("north::it", "fleet_relay")].into());
}

#[test]
fn a_selection_miss_edge_selects_its_test_for_the_file_it_guards() {
    let graph = graph(&[(GATE_ROWS, "in-binary", "north-fleet-supervisor"), (GATE_ROWS, "read-set", "set-1"), (GATE_ROWS, "guards", "harness/gates.toml"), ("reads:north", "reads-commit", "bde3db9c6")]);
    let facts = Facts::read(&graph, "north").unwrap();
    let cone = cone::of(&facts, &changed(&["harness/gates.toml"], Hunks::new()), None).unwrap();
    assert_eq!(cone.tests.into_iter().collect::<Vec<_>>(), vec![id("north-fleet-supervisor", "gate::gate_rows")], "rule 2 of docs/rationale/test-selection.md: a (test :guards FILE) fact selects the test whenever the lane changes FILE");
}

#[test]
fn a_function_changed_since_the_map_selects_the_tests_that_reached_it() {
    let graph = graph(&[
        ("test:north::it::fleet_relay", "in-binary", "north::it"),
        ("test:north::it::agent_text_reads", "in-binary", "north::it"),
        ("fn:crates/north-fleet/src/fleet/upward.rs:5-30", "in-file", "crates/north-fleet/src/fleet/upward.rs"),
        ("fn:crates/north-fleet/src/fleet/upward.rs:40-60", "in-file", "crates/north-fleet/src/fleet/upward.rs"),
        ("test:north::it::fleet_relay", "reaches", "fn:crates/north-fleet/src/fleet/upward.rs:5-30"),
        ("test:north::it::agent_text_reads", "reaches", "fn:crates/north-fleet/src/fleet/upward.rs:40-60"),
    ]);
    let facts = Facts::read(&graph, "north").unwrap();
    let since = parse("diff --git a/crates/north-fleet/src/fleet/upward.rs b/crates/north-fleet/src/fleet/upward.rs\n--- a/crates/north-fleet/src/fleet/upward.rs\n+++ b/crates/north-fleet/src/fleet/upward.rs\n@@ -12 +12 @@\ndiff --git a/crates/north-fleet/src/fleet/new.rs b/crates/north-fleet/src/fleet/new.rs\n--- /dev/null\n+++ b/crates/north-fleet/src/fleet/new.rs\n@@ -0,0 +1,9 @@\n");
    assert_eq!(cone::since_map(&facts, &since).unwrap().into_iter().collect::<Vec<_>>(), vec![id("north::it", "fleet_relay")], "a landed lane after the map commit changed a function fleet_relay reached, so a later lane cannot prove fleet_relay unaffected; agent_text_reads reached only an unchanged function, and a file no test reached at the map commit selects nothing");
}

#[test]
fn a_bare_selection_miss_edge_selects_its_test_in_whichever_binary_holds_it() {
    let graph = graph(&[(GATE_ROWS, "in-binary", "north-fleet-supervisor"), (GATE_ROWS, "read-set", "set-1"), ("test:gate::gate_rows", "guards", "harness/gates.toml"), ("test:worker_sandbox::confined", "guards", "harness/gates.toml"), ("reads:north", "reads-commit", "bde3db9c6")]);
    let facts = Facts::read(&graph, "north").unwrap();
    let cone = cone::of(&facts, &changed(&["harness/gates.toml"], Hunks::new()), None).unwrap();
    assert_eq!(cone.tests.into_iter().collect::<Vec<_>>(), vec![id("north-fleet-supervisor", "gate::gate_rows")], "north land missed tells test:NAME when no map holds the binary; a known name resolves to its binary");
    assert_eq!(cone.added, vec![cone::Added { package: String::new(), name: "worker_sandbox::confined".to_owned() }], "an unknown bare name runs by name in every binary rather than as binary worker_sandbox");
}
