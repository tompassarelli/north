use std::fs;

use super::{Fixture, git, north_enqueue_with, written};

fn lane_with_clippy(fixture: &Fixture, clean: bool) -> std::path::PathBuf {
    let runs = fixture.root.join("clippy-runs");
    let command = format!("echo ran >>{} && test -e clippy-clean # cargo clippy --all-targets -- -D warnings", runs.display());
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\tafter = \"** :: {command}\"\n")).unwrap();
    if clean {
        fs::write(fixture.repo.join("clippy-clean"), "clean\n").unwrap();
    }
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check"]);
    runs
}

fn runs(path: &std::path::Path) -> usize {
    fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

#[test]
fn enqueue_refuses_a_lane_failing_clippy_on_its_changed_crates_and_reuses_a_pass_for_the_same_tree() {
    let refused = Fixture::new();
    let counter = lane_with_clippy(&refused, false);
    let output = north_enqueue_with(&refused, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("a check before queueing exited") && stderr.contains("to rerun clippy alone: cargo clippy --all-targets --workspace -- -D warnings"), "{stderr}");
    assert_eq!(runs(&counter), 1, "{stderr}");
    assert!(written(&refused.state).iter().all(|path| path.extension().is_none_or(|extension| extension != "json")), "a refused lane writes no ticket: {:?}", written(&refused.state));

    let passed = Fixture::new();
    let counter = lane_with_clippy(&passed, true);
    let first = north_enqueue_with(&passed, &[]);
    let first_log = format!("{}{}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    assert!(!first_log.contains("a check before queueing exited"), "{first_log}");
    assert_eq!(runs(&counter), 1, "{first_log}");
    let second = north_enqueue_with(&passed, &[]);
    let second_log = format!("{}{}", String::from_utf8_lossy(&second.stdout), String::from_utf8_lossy(&second.stderr));
    assert!(second_log.contains("reused from the verdict cache"), "{second_log}");
    assert_eq!(runs(&counter), 1, "a cached pass for the same tree runs clippy no second time: {second_log}");
    a_lane_paying_an_over_ceiling_debt_goal_is_queued_urgent_and_any_other_lane_is_queued_in_order();
    enqueue_refuses_a_lane_whose_check_before_queueing_reached_no_verdict();
    enqueue_sorts_an_unsorted_block_into_the_lane_and_leaves_a_stale_index_to_the_lander_while_landing_refusals_is_shadow();
    enqueue_compiles_on_the_queueing_host_when_no_build_host_has_room_and_refuses_a_failure_there();
    enqueue_runs_an_added_check_whose_checker_the_live_runtime_does_not_cover_on_north_built_from_the_lane();
}

fn enqueue_refuses_a_lane_whose_check_before_queueing_reached_no_verdict() {
    let fixture = Fixture::new();
    let command = "exit 75 # cargo clippy --all-targets -- -D warnings";
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\tafter = \"** :: {command}\"\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check no host can run"]);
    let output = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success(), "{log}");
    assert!(log.contains("reached no verdict") && log.contains("not queued uncompiled"), "{log}");
    assert!(!written(&fixture.state).iter().any(|path| path.extension().is_some_and(|extension| extension == "json")), "{:?}", written(&fixture.state));
}

fn enqueue_compiles_on_the_queueing_host_when_no_build_host_has_room_and_refuses_a_failure_there() {
    let fixture = Fixture::new();
    let counter = fixture.root.join("clippy-runs");
    let hostless = fixture.root.join("orchestration.toml");
    fs::write(&hostless, "").unwrap();
    let command = format!("echo ran >>{} && exit 101 # cargo clippy --all-targets -- -D warnings", counter.display());
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n\trunner = build-host\n\tafter = \"** :: {command}\"\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check that runs on a build host"]);
    let output = north_enqueue_with(&fixture, &[("AGENTS_ORCHESTRATION", &hostless)]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success() && log.contains("a check before queueing exited 101"), "{log}");
    assert!(log.contains("no build host has room") && log.contains("run on this host"), "{log}");
    assert_eq!(runs(&counter), 1, "{log}");
    assert!(!written(&fixture.state).iter().any(|path| path.extension().is_some_and(|extension| extension == "json")), "{:?}", written(&fixture.state));
}

fn a_lane_paying_an_over_ceiling_debt_goal_is_queued_urgent_and_any_other_lane_is_queued_in_order() {
    let goals = "[[concern]]\nname = \"slow\"\nsource = \"README.md: lands fast\"\noracle = \"echo 90\"\nunit = \"s\"\nmax = 20\nclass = \"debt\"\nceiling = 60\nowner = \"north#641\"\nrevisit = \"a week over\"\npays = [\"crates/\"]\n";
    let urgent = |path: &str| {
        let fixture = Fixture::seeded(&[("file", "base\n"), ("concerns.toml", goals)]);
        fs::create_dir_all(fixture.repo.join(path).parent().unwrap()).unwrap();
        fixture.commit(&fixture.repo, path, "Add a file");
        let output = north_enqueue_with(&fixture, &[]);
        let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "a debt over its ceiling refuses no lane: {log}");
        let ticket = written(&fixture.state).into_iter().find(|path| path.extension().is_some_and(|extension| extension == "json")).expect("a ticket");
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(ticket).unwrap()).unwrap()["urgent"].as_str().map(str::to_owned)
    };
    assert!(urgent("crates/pay/file").is_some_and(|why| why.starts_with("slow reads 90 s")), "the lane that pays goes first");
    assert_eq!(urgent("docs/elsewhere"), None, "any other lane keeps its place");
}

fn enqueue_sorts_an_unsorted_block_into_the_lane_and_leaves_a_stale_index_to_the_lander_while_landing_refusals_is_shadow() {
    let first = "- A short entry.\n";
    let long = |word: &str| format!("- {}\n", format!("{word} ").repeat(100));
    let sets = "[[set]]\npath = \"docs/features.md\"\nkind = \"regenerate\"\ncommand = \"north docs features-index\"\nsources = [\"docs/features/\"]\n\n[[set]]\npath = \"list.txt\"\nkind = \"block\"\nstart = \"BEGIN\"\nend = \"END\"\nsorted = true\n";
    let fixture = Fixture::seeded(&[
        ("docs/features/_header.md", "# Features\n"),
        ("docs/features/01-first/_title.md", "## First\n"),
        ("docs/features/01-first/0001-short.md", first),
        ("docs/features.md", "# Features\n\n## First\n\n- A short entry.\n"),
        ("harness/merge-sets.toml", sets),
        ("list.txt", "BEGIN\na\nc\nEND\n"),
    ]);
    let unmarked = fixture.root.join("unmarked-runs");
    let lines = format!("[land]\n\tafter = \"docs/features/,docs/features.md :: $NORTH_LAND_NORTH docs features-index --check\"\n\tafter = \"@enqueue,docs/,list.txt :: ! grep -rq BROKEN docs/features && sed -n '/^BEGIN$/,/^END$/{{//!p}}' list.txt | LC_ALL=C sort -c -u\"\n\tafter = \"docs/ :: echo ran >>{} && exit 1\"\n", unmarked.display());
    fs::write(fixture.repo.join(".safe-push"), lines).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add the features index check"]);
    git(&fixture.repo, &["push", "-q", "origin", "lane:main"]);
    fs::write(fixture.repo.join("docs/features/01-first/0002-long.md"), long("BROKEN")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a broken features entry"]);
    let output = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && !log.contains("a check before queueing exited"), "an enqueue-marked after line refused the lane while landing_refusals is shadow: {log}");
    assert_eq!(runs(&unmarked), 0, "a line without the enqueue marker ran before queueing: {log}");
    fs::write(fixture.repo.join("docs/features/01-first/0002-long.md"), long("word")).unwrap();
    fs::write(fixture.repo.join("list.txt"), "BEGIN\na\nc\nb\nEND\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Fix the entry and add b, leaving the index stale and the list out of order"]);
    let output = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "a stale index or an unsorted block refused the lane: {log}");
    assert_eq!(git(&fixture.repo, &["show", "HEAD:list.txt"]), "BEGIN\na\nb\nc\nEND", "{log}");
    assert!(!git(&fixture.repo, &["show", "HEAD:docs/features.md"]).contains("word word"), "enqueue folded the generated index into the lane, so a worker's diff touches it: {log}");
    assert_eq!(runs(&unmarked), 0, "a line without the enqueue marker ran before queueing: {log}");

    fs::write(fixture.repo.join("docs/features.md"), "# Features\n\n## First\n\n- A short entry.\n- word word\n").unwrap();
    git(&fixture.repo, &["add", "docs/features.md"]);
    git(&fixture.repo, &["commit", "-qm", "Hand-edit the generated index"]);
    let output = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success() && log.contains("docs/features.md is generated from docs/features/"), "a lane that commits the generated index was not refused: {log}");
}

fn enqueue_runs_an_added_check_whose_checker_the_live_runtime_does_not_cover_on_north_built_from_the_lane() {
    let fixture = Fixture::new();
    let counter = fixture.root.join("built-runs");
    fs::create_dir_all(fixture.repo.join("bin")).unwrap();
    north_core::foundation::shim::write_executable(&fixture.repo.join("bin/head-north"), format!("#!/usr/bin/env bash\n[ \"$3\" = --no-build ] && {{ echo 'head-north: refused, no live runtime covers the checker code' >&2; exit 1; }}\necho ran >>{}\n", counter.display()));
    fs::write(fixture.repo.join(".safe-push"), "[land]\n\tcheck = \"** :: ./bin/head-north --code src/checker.rs --no-build docs check\"\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a check whose checker no live runtime covers"]);
    let output = north_enqueue_with(&fixture, &[]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && !log.contains("a land check this lane adds exited"), "{log}");
    assert_eq!(runs(&counter), 1, "{log}");
}

#[test]
fn enqueue_reruns_a_check_before_queueing_on_another_build_host_when_the_first_host_drops_its_connection() {
    let fixture = Fixture::new();
    let hosts = fixture.root.join("offload-hosts");
    let stub = fixture.root.join("north-stub");
    let script = format!(
        "case \"$1 $2 $3\" in\n  'capacity offload choose') case \"$*\" in *'--avoid flaky'*) echo steady ;; *) echo flaky ;; esac; exit 0 ;;\n  'capacity offload verdict') exit 0 ;;\n  'capacity offload run') printf '%s\\n' \"$5\" >>{hosts}; [ \"$5\" != flaky ] || {{ echo 'north capacity offload: the connection ended (ssh exit 255) before the remote host reported the command exit, so the run reached no verdict' >&2; exit 75; }}; exit 0 ;;\nesac\ncase \"$1 $2\" in 'capacity deadline') shift 4; [ \"$1\" != -- ] || shift; exec \"$@\" ;; esac\nexec {real} \"$@\"\n",
        hosts = hosts.display(),
        real = env!("CARGO_BIN_EXE_north")
    );
    north_core::foundation::shim::write_script(&stub, &script);
    fs::write(fixture.repo.join(".safe-push"), "[land]\n\trunner = build-host\n\tafter = \"** :: true # cargo clippy --all-targets -- -D warnings\"\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add a clippy after-landing check that runs on a build host"]);
    let output = north_enqueue_with(&fixture, &[("NORTH_LAND_NORTH", &stub)]);
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "{log}");
    assert_eq!(fs::read_to_string(&hosts).unwrap().lines().collect::<Vec<_>>(), ["flaky", "steady"], "on 2026-11-08 cpx62's ssh dropped mid-clippy and the lane was refused with exit 75 instead of rerunning the check on a host that answers: {log}");
    assert!(written(&fixture.state).iter().any(|path| path.extension().is_some_and(|extension| extension == "json")), "{:?}", written(&fixture.state));
}
