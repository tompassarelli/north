use std::path::Path;

use north_graph::{Graph, Query, Term, parse_query};
use north_github_mirror::store::Mirror;
use rusqlite::Connection;
use serde_json::json;

use crate::{Places, Source, ingest};

const FULL: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
const OTHER: &str = "0aa2d15297f1a6a2be5ba41e5bbf4c8a9d3a2f10";

fn body(first_ticked: bool) -> String {
    let mark = if first_ticked { "x" } else { " " };
    format!(
        "- [{mark}] A hung slot job is killed (owner: n634-hung-slots; landed 59ff4fb9c)\n- [x] The gate stops waiting (owner: n634-no-laptop-cargo; landed 0aa2d1529)\n- [x] Nobody ran this one (owner: n634-ghost)\n- [ ] Open work (owner: unassigned)\n"
    )
}

fn tick(mirror: &Mirror, first_ticked: bool) {
    let issue = json!({"number": 634, "title": "Verification", "body": body(first_ticked), "labels": {"nodes": [{"name": "bet"}]}, "blockedBy": {"nodes": []}});
    mirror.write("north/issues/open", &json!([issue]), None, 1.0).unwrap();
    mirror.write("north/issues/closed", &json!({"numbers": [7]}), None, 1.0).unwrap();
}

fn places(directory: &Path) -> Places {
    let mirror = Mirror::at(directory.join("mirror.db"));
    tick(&mirror, true);
    let ledger = directory.join("ledger.db");
    Connection::open(&ledger)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE checks(check_id INTEGER PRIMARY KEY, task_ref TEXT NOT NULL, command TEXT NOT NULL, cwd TEXT NOT NULL, head_sha TEXT, exit INTEGER NOT NULL, ran_ms INTEGER NOT NULL);
             CREATE TABLE tasks(task_ref TEXT PRIMARY KEY, category TEXT, sessions TEXT NOT NULL DEFAULT '[]', wall_ms INTEGER, outcome TEXT, landed_sha TEXT, review TEXT, corrections INTEGER NOT NULL DEFAULT 0, landed_ms INTEGER);
             INSERT INTO checks VALUES (1, 'tompassarelli/north#634', 'safe-push landing', '', '{FULL}', 0, 1);
             INSERT INTO tasks (task_ref, sessions, landed_sha) VALUES ('north#634', '[\"n634-hung-slots\"]', '{OTHER}');"
        ))
        .unwrap();
    std::fs::write(directory.join("pushes.tsv"), format!("1793926432\tnorth\tn634-hung-slots\t{FULL}\t{FULL}\tn634-hung-slots\n")).unwrap();
    std::fs::write(directory.join("corrections.jsonl"), "{\"id\":\"L1\",\"at\":\"2026-10-01T14:15:12Z\",\"kind\":\"correction\",\"quote\":\"Tom said the reply did not answer his question.\",\"source\":\"tom convo:1 session:abc\",\"superseded_by\":null,\"class\":\"unanswered\"}\n").unwrap();
    std::fs::create_dir_all(directory.join("coverage")).unwrap();
    Places { mirror, ledger, threads: directory.join("threads.db"), pushes: directory.join("pushes.tsv"), code: directory.join("code"), corrections: directory.join("corrections.jsonl"), coverage: directory.join("coverage"), ranking: directory.join("backlog-ranking.md"), land_reuse: directory.join("land-reuse"), verdict_cache: directory.join("verdict-cache") }
}

fn unobserved_claims() -> Query {
    let claim = json!({"head": {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]}, "body": [
        {"rel": "assertion", "args": [{"var": "who"}, {"var": "speaker"}, ["keyword", "claims"], {"var": "q"}]},
        {"pred": "eq", "args": [{"var": "who"}, {"var": "speaker"}]},
        {"rel": "quoted", "args": [{"var": "q"}, {"var": "box"}, ["keyword", "done-at"], {"var": "sha"}]},
        {"rel": "triple", "args": ["north#634", ["keyword", "has-box"], {"var": "box"}]}]});
    let observed = json!({"head": {"rel": "observed", "args": [{"var": "sha"}]}, "body": [
        {"rel": "assertion", "args": ["checker", {"var": "sha"}, ["keyword", "check-passed"], {"var": "command"}]}]});
    let unobserved = json!({"head": {"rel": "unobserved", "args": [{"var": "who"}, {"var": "sha"}]}, "body": [
        {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]},
        {"rel": "observed", "args": [{"var": "sha"}], "neg": true}]});
    parse_query(&json!({"find": "unobserved", "strata": [[claim, observed], [unobserved]]}).to_string()).unwrap()
}

fn unobserved(graph: &Graph) -> Vec<(Term, Term)> {
    graph.ask(&unobserved_claims()).unwrap().rows.into_iter().map(|row| (row[0].clone(), row[1].clone())).collect()
}

#[test]
fn a_done_claim_is_the_workers_quoted_triple_only_the_checker_can_observe_and_a_rerun_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Mirror, Source::Ledger, Source::Pushes]);
    assert!(!report.failed, "{:?}", report.lines);
    let mut found = unobserved(&graph);
    found.sort();
    assert_eq!(
        found,
        [(Term::string("n634-ghost"), Term::keyword("none")), (Term::string("n634-no-laptop-cargo"), Term::string(OTHER))],
        "the claim whose commit the checker passed is observed; a claim naming no commit, and one the checker never saw, are not"
    );
    let told = graph.told().unwrap();
    assert!(
        !told.iter().any(|fact| fact.triple.t2 == Term::keyword("done-at")),
        "the commit a box names is quoted inside the worker's claim and is never a bare fact"
    );
    a_run_claims_its_box_of_a_closed_issue_and_the_lander_observes_the_landing(&places, &mut graph);
    a_fleet_session_cannot_tell_under_a_trusted_name();
    a_rerun_adds_nothing_and_a_changed_source_is_followed();
}

fn a_fleet_session_cannot_tell_under_a_trusted_name() {
    use crate::trusted_writers::refusal;
    let session = Some("0::/user.slice/north-session-7.scope/inner\n");
    let broker = Some("0::/user.slice/north-worker-outside-1.service\n");
    for writer in ["checker", "mirror", "lander", "git"] {
        assert!(refusal("tell", writer, session).is_some() && refusal("retract", writer, broker).is_some(), "{writer}");
    }
    assert!(refusal("tell", "checker", Some("0::/user.slice/north-lander.service\n")).is_none() && refusal("tell", "checker", None).is_some());
    assert!(refusal("show", "checker", session).is_none() && refusal("tell", "selection-miss", session).is_none());
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("graph.db").display().to_string();
    let words = ["tell", "--db", &database, "--writer", "checker", "--file", "--writer", "S", "check-passed"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| refusal(verb, writer, session));
    assert_eq!(code, 1, "the writer the parser resolved is the one refused, since --file takes the later --writer as its value");
    assert!(!directory.path().join("graph.db").exists());
    let seen = std::cell::RefCell::new(Vec::new());
    let words = ["tell", "--db", &database, "--writer", "S", "--file", "--writer", "checker"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| {
        seen.borrow_mut().push(writer.to_owned());
        refusal(verb, writer, session)
    });
    assert_eq!(*seen.borrow(), ["S"], "the parser resolves S, so a reading of the raw words that refuses checker here is the wrong one");
    assert_eq!(code, 2, "the guard let it through and the write failed on its arguments");
}

fn a_run_claims_its_box_of_a_closed_issue_and_the_lander_observes_the_landing(places: &Places, graph: &mut Graph) {
    Connection::open(&places.threads)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE runs(agent TEXT, item TEXT, box TEXT, landed_sha TEXT, outcome TEXT, role TEXT, category TEXT, brief_text TEXT, minutes REAL);
             INSERT INTO runs VALUES ('run-7', 'north#7', 'Ship the closed box. Done when: `cargo test -p x` exits 0', '{FULL}', 'done', 'worker', 'feature', '', 30);"
        ))
        .unwrap();
    let report = ingest(graph, places, &[Source::Mirror, Source::Observations]);
    assert!(!report.failed, "{:?}", report.lines);
    let node = Term::string("box:north#7:ship-the-closed-box-done-when-cargo-test-p-x-exi");
    let told = graph.told().unwrap();
    assert!(told.contains(&north_graph::Told::new("run-7", "run-7", Term::keyword("claims"), Term::triple(node.clone(), Term::keyword("done-at"), Term::string(FULL)))), "a done run claims its box though the issue is closed: {:?}", report.lines);
    assert!(told.contains(&north_graph::Told::new("ledger", node, Term::keyword("done-when"), Term::string("cargo test -p x"))));
    assert!(told.contains(&north_graph::Told::new("lander", FULL, Term::keyword("landed-on"), Term::string("main"))), "{:?}", report.lines);
    let again = ingest(graph, places, &[Source::Observations]);
    assert!(again.lines.iter().any(|line| line.starts_with("observations: 0 asserted")), "{:?}", again.lines);
}

fn a_rerun_adds_nothing_and_a_changed_source_is_followed() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let mut graph = Graph::open_in_memory().unwrap();
    let all = [Source::Mirror, Source::Ledger, Source::Pushes, Source::Corrections];
    ingest(&mut graph, &places, &all);
    let before = graph.validate().unwrap().occurrences;
    let again = ingest(&mut graph, &places, &all);
    assert_eq!(graph.validate().unwrap().occurrences, before, "{:?}", again.lines);
    assert!(again.lines.iter().all(|line| line.contains("0 asserted, 0 retracted")), "{:?}", again.lines);
    let claims = |graph: &Graph| graph.told().unwrap().iter().filter(|fact| fact.triple.t2 == Term::keyword("claims")).count();
    assert_eq!(claims(&graph), 3);
    tick(&places.mirror, false);
    ingest(&mut graph, &places, &all);
    assert_eq!(claims(&graph), 2, "unticking the box withdraws its owner's claim");
    assert!(graph.validate().unwrap().is_sound());
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.com"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_landed_commit_is_told_to_have_touched_the_files_it_changed() {
    let directory = tempfile::tempdir().unwrap();
    let mut places = places(directory.path());
    let checkout = directory.path().join("code/north/main");
    std::fs::create_dir_all(checkout.join("crates/x")).unwrap();
    git(&checkout, &["init", "-q"]);
    std::fs::write(checkout.join("crates/x/lib.rs"), "a").unwrap();
    std::fs::write(checkout.join("README.md"), "a").unwrap();
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "one"]);
    let landed = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("README.md"), "b").unwrap();
    git(&checkout, &["commit", "-q", "-am", "never pushed"]);
    let now = north_base::foundation::clock::Timestamp::now().unix_seconds();
    std::fs::write(&places.pushes, format!("{now}\tnorth\tlane\t{landed}\t{landed}\tn669-grounding\n")).unwrap();
    places.code = directory.path().join("code");
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Touches]);
    assert!(!report.failed, "{:?}", report.lines);
    let mut touched: Vec<String> = graph.mentions(&Term::string(landed.as_str()), 10).unwrap().into_iter().filter(|fact| fact.triple.t2 == Term::keyword("touched")).map(|fact| fact.triple.t3.to_string()).collect();
    touched.sort();
    assert_eq!(touched, ["\"README.md\"", "\"crates/x/lib.rs\""], "only a commit the push log landed is read, with the files that commit changed");
}

fn told_as(graph: &Graph, predicate: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = graph.told().unwrap().iter().filter(|fact| fact.triple.t2 == Term::keyword(predicate)).map(|fact| (fact.triple.t1.to_string(), fact.triple.t3.to_string())).collect();
    found.sort();
    found
}

#[test]
fn box_waits_and_the_ranking_documents_order_and_edges_reach_the_graph_and_unreadable_waits_are_named() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let body = "- [ ] Ship the gateway\n- [ ] Move the callers (after north#560; waits on the broker box)\n";
    let other = "Follows: north#7\n- [ ] Cull the tests\n";
    let issues = [json!({"number": 7, "title": "Gateway", "body": body, "labels": {"nodes": []}, "blockedBy": {"nodes": []}}), json!({"number": 8, "title": "Cull", "body": other, "labels": {"nodes": []}, "blockedBy": {"nodes": []}})];
    places.mirror.write("north/issues/open", &json!(issues), None, 1.0).unwrap();
    std::fs::write(&places.ranking, "## Ranked list\n\n1. north#8 Cull first\n2. north#7 Gateway\n\n## DAG\n\n- north#7 box 1 -> north#8\n- each fix -> its observation box\n").unwrap();
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Mirror, Source::Ranking]);
    assert!(!report.failed, "{:?}", report.lines);
    assert_eq!(told_as(&graph, "rank"), [("\"north#7\"".to_owned(), "2".to_owned()), ("\"north#8\"".to_owned(), "1".to_owned())]);
    assert_eq!(told_as(&graph, "follows"), [("\"north#8\"".to_owned(), "\"north#7\"".to_owned())]);
    let waits: Vec<String> = told_as(&graph, "waits-on").into_iter().map(|(_, target)| target).collect();
    assert_eq!(waits, ["\"north#560\""], "the issue the box names is an edge; the broker box named in words is not");
    let first_box = crate::boxes::parse(body)[0].id("north#7");
    let doc = told_as(&graph, "doc-waits-on");
    assert_eq!(doc, [("\"north#8\"".to_owned(), format!("\"{first_box}\""))]);
    let named = report.lines.join("\n");
    assert!(named.contains("mirror: cannot parse a wait of north#7:") && named.contains("the broker box"), "{named}");
    assert!(named.contains("ranking: cannot parse a wait in backlog-ranking.md: each fix -> its observation box"), "{named}");
}

const DEBT_AT_1022: &str = "Category: bug-known-cause
Class: a gate that runs after landing failed on a landed commit: every section below.
Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit.

## Done when
- [ ] No section remains below: each failed gate is fixed forward on main and its section deleted.WAITS
- [ ] **Split north-ops to about 60% of its line cap.** north-ops is over its 25000-line cap. (detector crate-headroom:north-ops:25000) Tracked in north#743.
- [ ] **Split north-guard to about 60% of its line cap.** north-guard has 24477 non-test lines against its 25000-line cap. (detector crate-headroom:north-guard:25000) Tracked in north#744.
- [ ] **Split north-landing to about 60% of its line cap.** north-landing is over its 10000-line cap. (detector crate-headroom:north-landing:10000) Tracked in north#745.
- [ ] **Split north-fleet to about 60% of its line cap.** north-fleet has 9929 non-test lines against its 10000-line cap. (detector crate-headroom:north-fleet:10000) Tracked in north#746.

<!-- after-landing-gate: ff1a537b8491 crate-lines -->
### ff1a537b8491 (2026-11-07) gates: GATES

crate-lines
```
✗ crate-lines: north-landing grew 22621 -> 22635 non-test lines, over its cap of 10000 (4 dependents)
after-landing: ✗ crate-lines gate refused: a crate over its cap grew.
```
";

fn backlog_of(body: &str) -> north_graph_ground::Backlog {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let split = |number: u64| json!({"number": number, "title": format!("Split crate {number}"), "body": "- [ ] Split it\n", "labels": {"nodes": []}, "blockedBy": {"nodes": []}});
    let debt = json!({"number": 722, "title": "After-landing gate debt", "body": body, "labels": {"nodes": []}, "blockedBy": {"nodes": []}});
    places.mirror.write("north/issues/open", &json!([debt, split(743), split(744), split(745), split(746)]), None, 1.0).unwrap();
    let path = directory.path().join("graph.db");
    let mut graph = Graph::open(&path).unwrap();
    let report = ingest(&mut graph, &places, &[Source::Mirror]);
    assert!(!report.failed, "{:?}", report.lines);
    drop(graph);
    north_graph_ground::read_backlog_from(&path).unwrap()
}

#[test]
fn a_debt_issue_whose_every_section_waits_on_a_crate_split_is_held_behind_the_splits_and_one_with_a_fixable_section_is_ready() {
    use north_graph_ground::Verdict;
    let held = backlog_of(&DEBT_AT_1022.replace("WAITS", " Waits on boxes 2, 3, 4 and 5.").replace("GATES", "crate-lines"));
    let Verdict::Held(why) = held.verdict("north#722") else { panic!("every section waits on a split issue: {:?}", held.ready) };
    let edges: Vec<&str> = held.held.iter().filter(|held| held.entry.issue == "north#722").map(|held| held.edge.as_str()).collect();
    for split in ["north#743", "north#744", "north#745", "north#746"] {
        assert!(edges.contains(&split), "{split} missing from {edges:?}: {why}");
    }
    assert!(held.held.iter().any(|held| held.entry.text.starts_with("No section remains below") && held.edge_text.contains("Split north-ops")), "the first box waits on the split boxes: {:?}", held.held);

    let fixable = backlog_of(&DEBT_AT_1022.replace("WAITS", "").replace("GATES", "crate-lines, code-health"));
    assert!(matches!(fixable.verdict("north#722"), Verdict::Ready), "a code-health section can be fixed forward: {:?}", fixable.held);
}

#[test]
fn crates_reads_the_cargo_path_dependency_shapes_and_embeds_at_origin_main_not_the_working_tree() {
    let directory = tempfile::tempdir().unwrap();
    let mut places = places(directory.path());
    places.code = directory.path().join("code");
    let checkout = places.code.join("north/main");
    let files = [
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\"]\n[workspace.dependencies]\nc = { path = \"crates/c\" }\nserde = \"1\"\n[package]\nname = \"top\"\n[dependencies]\na = { path = \"crates/a\" }\n"),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\" }\nserde.workspace = true\noutside = { path = \"../../../elsewhere\" }\n[dev-dependencies]\nc.workspace = true\n"),
        ("crates/b/Cargo.toml", "[package]\nname = \"b\"\n[build-dependencies.c]\npath = \"../c\"\n"),
        ("crates/c/Cargo.toml", "[package]\nname = \"c\"\n"),
        ("crates/a/src/lib.rs", "const A: &str = include_str!(\"../data/a.toml\");\nconst B: &[u8] = include_bytes!(\n    concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../harness/defaults.toml\"),\n);\nconst C: &str = \"include_str!(\\\"/etc/hostname\\\")\";\n"),
    ];
    for (path, body) in files {
        std::fs::create_dir_all(checkout.join(path).parent().unwrap()).unwrap();
        std::fs::write(checkout.join(path), body).unwrap();
    }
    git(&checkout, &["init", "-q"]);
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "one"]);
    let landed = git(&checkout, &["rev-parse", "HEAD"]);
    git(&checkout, &["update-ref", "refs/remotes/origin/main", &landed]);
    git(&checkout, &["checkout", "-q", "-b", "elsewhere"]);
    std::fs::write(checkout.join("crates/c/Cargo.toml"), "[package]\nname = \"c\"\n[dependencies]\na = { path = \"../a\" }\n").unwrap();
    git(&checkout, &["commit", "-q", "-am", "never landed"]);
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Crates]);
    assert!(!report.failed, "{:?}", report.lines);
    let pairs = |pairs: &[(&str, &str)]| pairs.iter().map(|(subject, object)| (format!("\"{subject}\""), format!("\"{object}\""))).collect::<Vec<_>>();
    assert_eq!(told_as(&graph, "root"), pairs(&[("crate:a", "crates/a"), ("crate:b", "crates/b"), ("crate:c", "crates/c"), ("crate:top", ".")]));
    assert_eq!(told_as(&graph, "depends-on"), pairs(&[("crate:a", "crate:b"), ("crate:a", "crate:c"), ("crate:b", "crate:c"), ("crate:top", "crate:a")]), "every path dependency on a member, of any kind and in any shape, at origin/main");
    assert_eq!(told_as(&graph, "embeds"), pairs(&[("crates/a/src/lib.rs", "crates/a/data/a.toml"), ("crates/a/src/lib.rs", "harness/defaults.toml")]));
    assert_eq!(told_as(&graph, "crates-commit"), pairs(&[("crates:north", &landed)]));
    let again = ingest(&mut graph, &places, &[Source::Crates]);
    assert_eq!(again.lines, ["crates: 0 asserted, 0 retracted, 11 unchanged"]);
}

#[test]
fn reads_takes_each_tests_newest_shadow_unit_line_naming_a_set_and_tells_its_path_set() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Reads]);
    assert!(!report.failed && report.lines[1].contains("no shadow log"), "{:?}", report.lines);
    std::fs::create_dir_all(&places.land_reuse).unwrap();
    let sets = places.verdict_cache.join("v2/0a1b/sets");
    std::fs::create_dir_all(&sets).unwrap();
    std::fs::write(sets.join("aa11"), "content\tharness/old.toml\n").unwrap();
    std::fs::write(sets.join("bb22"), "absent\tharness/local.toml\ncontent\tharness/defaults.toml\nexec\tbin/safe-push\nlisting\t.\nlisting\tcrates\ncontent\ttarget/debug/north\nunreusable\treads $HOME\n").unwrap();
    let test = "north-fleet-supervisor gate_rows::a_gate_row_reads_the_defaults";
    std::fs::write(
        places.land_reuse.join("2026-11-08.tsv"),
        format!("100\tunit\tp100\t{test}\taa11\tpass\t0.5\n200\tunit\tp200\t{test}\tbb22\tpass\t0.4\n300\tunit\tp300\t{test}\t-\tfail\t0.1\n250\tunit\tp250\tnorth::it stdio::lines\t-\tpass\t1.0\n"),
    )
    .unwrap();
    let report = ingest(&mut graph, &places, &[Source::Reads]);
    assert!(!report.failed, "{:?}", report.lines);
    let node = "\"test:north-fleet-supervisor::gate_rows::a_gate_row_reads_the_defaults\"";
    let of = |predicate: &str| told_as(&graph, predicate).into_iter().map(|(subject, object)| format!("{} {object}", if subject == node { "T" } else { &subject })).collect::<Vec<_>>();
    assert_eq!(of("reads"), ["T \"bin/safe-push\"", "T \"harness/defaults.toml\""], "content and exec of the newest line naming a set; nothing under target/");
    assert_eq!(of("lists"), ["T \".\"", "T \"crates\""]);
    assert_eq!(of("expects-absent"), ["T \"harness/local.toml\""]);
    assert_eq!(of("read-set"), ["T \"bb22\""], "one set per recorded test, and no fact for its environment-only lines");
    assert_eq!(of("recorded-in"), ["T \"north-fleet-supervisor\""], "the binary the shadow line names, so the test is known with no coverage");
    assert_eq!(of("reads-commit"), ["\"reads:north\" \"p300\""]);
    let again = ingest(&mut graph, &places, &[Source::Reads]);
    assert_eq!(again.lines, ["reads: 0 asserted, 0 retracted, 8 unchanged"]);
}

#[test]
fn a_leaf_is_a_node_with_its_parent_and_done_when() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let leaf = json!({"number": 900, "title": "Read a leaf", "body": "Read a leaf.\n\nDone when: `cargo test -p x` exits 0\n", "labels": {"nodes": []}, "blockedBy": {"nodes": [{"number": 901, "state": "OPEN", "repository": {"name": "north"}}]}, "parent": {"number": 899, "repository": {"name": "north"}}, "subIssuesSummary": {"total": 0, "completed": 0}});
    let parent = json!({"number": 899, "title": "An outcome", "body": "", "labels": {"nodes": []}, "blockedBy": {"nodes": []}, "parent": null, "subIssuesSummary": {"total": 1, "completed": 0}});
    let boxed = json!({"number": 902, "title": "Boxes", "body": "- [ ] A box\n", "labels": {"nodes": []}, "blockedBy": {"nodes": []}, "parent": {"number": 899, "repository": {"name": "north"}}, "subIssuesSummary": {"total": 0, "completed": 0}});
    places.mirror.write("north/issues/open", &json!([leaf, parent, boxed]), None, 1.0).unwrap();
    let mut graph = Graph::open_in_memory().unwrap();
    let report = ingest(&mut graph, &places, &[Source::Mirror]);
    assert!(!report.failed, "{:?}", report.lines);
    let told = graph.told().unwrap();
    let has = |subject: &str, predicate: &str, object: Term| told.contains(&north_graph::Told::new("mirror", subject, Term::keyword(predicate), object));
    assert!(has("north#900", "leaf", Term::boolean(true)));
    assert!(has("north#900", "parent", Term::string("north#899")));
    assert!(has("north#900", "done-when", Term::string("cargo test -p x")));
    assert!(has("north#900", "blocked-by", Term::string("north#901")));
    assert!(!has("north#899", "leaf", Term::boolean(true)), "a parent with an open child is not a leaf");
    assert!(!has("north#902", "leaf", Term::boolean(true)), "an issue with checkboxes is on the box path");
}
