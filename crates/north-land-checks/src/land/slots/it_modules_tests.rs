use super::*;
use super::super::tree_wide_units;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap()
}

fn selected(files: &[&str]) -> Selection {
    let files: Vec<String> = files.iter().map(|file| (*file).to_owned()).collect();
    select(&read(STRUCTURE), &read(TABLE), &spawners(root()), &files).unwrap()
}

#[test]
fn a_probe_through_src_and_four_crates_runs_only_the_tests_it_modules_their_crates_select() {
    let probe = ["src/cli/worker_ledger/recalibrate.rs", "crates/north-calibration/src/threads/calibration/box_eta.rs", "crates/north-capacity-host/src/capacity/command.rs", "crates/north-climb/src/arms.rs", "crates/north-context-pack/src/cochange.rs"];
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let table = Table::parse(&read(TABLE), &plan, &spawners(root())).unwrap();
    let affected = plan.closure(["north", "north-calibration", "north-capacity-host", "north-climb", "north-context-pack"].map(str::to_owned).into());
    let expected: BTreeSet<String> = table.modules().filter(|module| !table.crates(module).unwrap().is_disjoint(&affected)).map(str::to_owned).collect();
    let selection = selected(&probe);
    assert_eq!(selection, Selection::Modules { modules: expected, units: Units::default(), floor: Floor::default() }, "the probe runs the modules crates.txt maps to its crates");
    assert!(selection.arguments().starts_with("--tests -E kind(lib)|kind(bin)|(binary_id(north::it)&test(/"), "the root package's unit tests still run");
    let modules = |names: &[&str]| Selection::Modules { modules: names.iter().map(|name| (*name).to_owned()).chain(["affected_modules", "agent_text_reads"].map(str::to_owned)).collect(), units: Units::default(), floor: Floor::default() };
    assert_eq!(selected(&["tests/it/threads_ledger.rs"]), modules(&["threads_ledger"]), "an edited module runs with the tree-wide ones");
    assert_eq!(selected(&["tests/it/runtime_update/prune.rs"]), modules(&["runtime_update"]), "a file in a module's directory selects it");
    for file in ["tests/it/support/tree.rs", "tests/it/main.rs", TABLE, "examples/push_trace_git.rs", "Cargo.lock"] {
        assert_eq!(selected(&["src/main.rs", file]), Selection::Every, "{file} runs every module");
    }
}

fn graph_stub(directory: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = directory.join(name);
    north_base::foundation::shim::write_script(&path, &format!("{body}\n"));
    path
}

#[test]
fn a_lane_the_graph_traces_runs_only_its_tests_and_an_untraced_lane_runs_the_crate_level_selection() {
    let directory = tempfile::tempdir().unwrap();
    let answer = r#"{"tests":["north::it fleet::launch::refuses_a_full_window","north-land-checks land::slots::select::tests::queues","north-coverage::pick_tests pick::regions"],"always":["north-base::bin/probe tests::reads_home"],"added":[{"package":"north-land-checks","name":"narrows_to_ids"}],"files":[],"untraced":null}"#;
    let traced = graph_stub(directory.path(), "traced", &format!("printf '%s' '{answer}'"));
    let selection = selection_asking_with(&traced, root(), "HEAD", "HEAD", &Batch::default(), BTreeMap::new).unwrap();
    assert_eq!(
        selection.arguments(),
        "--tests -E (binary_id(north-base::bin/probe)&(test(=tests::reads_home)))|(binary_id(north-coverage::pick_tests)&(test(=pick::regions)))|(binary_id(north-land-checks)&(test(=land::slots::select::tests::queues)))|(binary_id(north::it)&(test(=fleet::launch::refuses_a_full_window)))|(package(north-land-checks)&test(/(^|::)narrows_to_ids$/))|(binary_id(north::it)&test(/^(affected_modules|agent_text_reads)::/))|(binary_id(=north-code-health)&test(=code_health::habits_tests::the_north_tree_keeps_its_rust_habits))|(binary_id(=north-runtime)&test(=runtime::reuse::tests::every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input))|(binary_id(=north-runtime)&test(=runtime::reuse::tests::no_source_or_test_embeds_a_table_the_runtime_reads_from_its_tree))",
        "a traced lane runs its traced and always tests in every test binary, integration binaries outside tests/it included, the tests it adds, and the tree-wide tests/it modules"
    );
    assert!(selection.runs_module("agent_text_reads") && selection.runs_module("fleet") && !selection.runs_module("safe_push"), "a traced lane keeps the tree-wide modules that coverage cannot link to a change (north#706)");
    assert_eq!(selection.packages(), ["north", "north-base", "north-code-health", "north-coverage", "north-land-checks", "north-runtime"].map(str::to_owned).into(), "the packages of the selected tests and of the tree-wide unit tests are built");
    let crate_level = select(&read(STRUCTURE), &read(TABLE), &spawners(root()), &[]).unwrap().with_units(units(&read(tree_wide_units::TABLE), &Plan::parse(&read(STRUCTURE)).unwrap()).unwrap());
    for (name, body) in [("untraced", "echo '{\"tests\":[]}'; exit 3"), ("missing", "exit 2")] {
        let program = graph_stub(directory.path(), name, body);
        assert_eq!(selection_asking_with(&program, root(), "HEAD", "HEAD", &Batch::default(), BTreeMap::new).unwrap(), crate_level, "a graph that answers {name} falls back to the crate-level selection");
    }
}

const TREE_WIDE_UNITS: [&str; 3] = [
    "code_health::habits_tests::the_north_tree_keeps_its_rust_habits",
    "runtime::reuse::tests::every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input",
    "runtime::reuse::tests::no_source_or_test_embeds_a_table_the_runtime_reads_from_its_tree",
];

#[test]
fn every_lane_selection_names_the_tree_wide_unit_tests_the_graph_and_the_crate_fallback_alike() {
    let directory = tempfile::tempdir().unwrap();
    let traced = graph_stub(directory.path(), "traced", r#"printf '%s' '{"tests":["north-land-checks land::slots::select::tests::queues"],"files":[]}'"#);
    let untraced = graph_stub(directory.path(), "untraced", "exit 2");
    let table = read(tree_wide_units::TABLE);
    for program in [&traced, &untraced] {
        let selection = selection_asking_with(program, root(), "HEAD", "HEAD", &Batch::default(), BTreeMap::new).unwrap();
        for test in TREE_WIDE_UNITS {
            assert!(selection.summary().contains(test), "{program:?}: the selection line names {test}: {}", selection.summary());
            assert!(table.contains(test), "{test} is declared in {}", tree_wide_units::TABLE);
        }
        assert!(selection.packages().is_superset(&["north-code-health".to_owned(), "north-runtime".to_owned()].into()), "{program:?}: their packages are built");
    }
    let graph = selection_asking_with(&traced, root(), "HEAD", "HEAD", &Batch::default(), BTreeMap::new).unwrap().arguments();
    for test in TREE_WIDE_UNITS {
        assert!(graph.contains(&format!("test(={test})")), "the graph filter runs {test}: {graph}");
    }
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let packages = plan.names().chain([ROOT]).collect();
    for bad in ["north-runtime runtime::reuse::tests::x\n", "no-such-crate a::b # tree-wide: why\n", "north-runtime a::b # tree-wide:\n", "north-runtime a::b # tree-wide: why\nnorth-runtime a::b # tree-wide: again\n"] {
        assert!(tree_wide_units::Units::parse(bad, &packages).is_err(), "{bad:?} is refused");
    }
}

#[test]
fn every_declared_tree_wide_unit_test_still_exists_in_its_package() {
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let units = tree_wide_units::Units::parse(&read(tree_wide_units::TABLE), &plan.names().chain([ROOT]).collect()).unwrap();
    for entry in units.names().split(", ") {
        let (package, test) = entry.split_once(' ').unwrap();
        let name = test.rsplit("::").next().unwrap();
        let mut pending = vec![root().join("crates").join(package).join("src")];
        let mut found = false;
        while let Some(path) = pending.pop() {
            if path.is_dir() {
                pending.extend(std::fs::read_dir(&path).unwrap().flatten().map(|entry| entry.path()));
            } else {
                found |= path.extension().is_some_and(|extension| extension == "rs") && std::fs::read_to_string(&path).unwrap().contains(&format!("fn {name}()"));
            }
        }
        assert!(found, "{package} declares no test fn {name}");
    }
}

#[test]
fn a_selected_integration_binary_of_another_crate_is_built_and_run() {
    let directory = tempfile::tempdir().unwrap();
    let traced = graph_stub(directory.path(), "traced", r#"printf '%s' '{"tests":["north-lander::it land_chaos::reverts_the_red_lane"],"files":[]}'"#);
    let selection = selection_asking_with(&traced, root(), "HEAD", "HEAD", &Batch::default(), BTreeMap::new).unwrap();
    let arguments = selection.arguments();
    assert!(arguments.starts_with("--tests -E "), "the tests/it binaries are built: {arguments}");
    assert!(arguments.contains("(binary_id(north-lander::it)&(test(=land_chaos::reverts_the_red_lane)))"), "the selected test runs: {arguments}");
    assert!(selection.packages().contains("north-lander"), "its package is built");
    assert!(Selection::Every.arguments().ends_with("|kind(test)"), "the whole-workspace run keeps every tests/it binary");
}

const SANDBOX_TEST: &str = "worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state";

fn git(repo: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn replay_of_the_sandbox_move(repo: &Path) -> (String, String) {
    for table in [STRUCTURE, TABLE, tree_wide_units::TABLE, super::super::guard_suites::TABLE] {
        std::fs::create_dir_all(repo.join(table).parent().unwrap()).unwrap();
        std::fs::copy(root().join(table), repo.join(table)).unwrap();
    }
    for entry in std::fs::read_dir(root().join("tests/it")).unwrap().flatten().filter(|entry| entry.path().is_file()) {
        std::fs::copy(entry.path(), repo.join("tests/it").join(entry.file_name())).unwrap();
    }
    let moved = ["except.rs", "executable.rs", "lane_only.rs", "readable.rs", "removable.rs"];
    let kept = ["crates/north-runtime/src/worker_sandbox/broker.rs", "crates/north-runtime/src/worker_sandbox/mod.rs", "crates/north-runtime/src/worker_sandbox/tests.rs", "crates/north-providers/src/providers/opencode/harness.rs", "crates/north-providers/src/providers/opencode/turns.rs", "crates/north-base/src/platform/mod.rs", "crates/north-base/Cargo.toml", "crates/north-land-sandbox/src/lib.rs", "crates/north-runtime/src/privacy/mod.rs", "crates/north-land-scans/src/outgoing/mod.rs", "crates/north-lander/src/lib.rs", "crates/north-land-checks/src/lib.rs", "crates/north-safe-push/src/lib.rs", "Cargo.lock", "docs/rationale/worker-sandbox.md"];
    let write = |path: &str, text: &str| {
        std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        std::fs::write(repo.join(path), text).unwrap();
    };
    for file in kept.iter().copied().chain(moved.iter().map(|name| format!("crates/north-runtime/src/worker_sandbox/{name}")).collect::<Vec<_>>().iter().map(String::as_str)) {
        write(file, &format!("// {file}\nfn policy() {{}}\n"));
    }
    for path in super::super::guard_suites::Suites::parse(&read(super::super::guard_suites::TABLE)).unwrap().paths() {
        if path.ends_with('/') {
            std::fs::create_dir_all(repo.join(path)).unwrap();
        } else if !repo.join(path).exists() {
            write(path, "fn guarded() {}\n");
        }
    }
    git(repo, &["init", "-q"]);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "base"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(repo.join("crates/north-base/src/platform/worker_sandbox")).unwrap();
    for name in moved {
        git(repo, &["mv", &format!("crates/north-runtime/src/worker_sandbox/{name}"), &format!("crates/north-base/src/platform/worker_sandbox/{name}")]);
    }
    for file in ["crates/north-base/src/platform/worker_sandbox/mod.rs", "crates/north-base/src/platform/worker_sandbox/tests.rs"] {
        write(file, "fn moved() {}\n");
    }
    for file in &kept[..7] {
        write(file, &format!("// {file}, now calling north-base\nfn policy() {{}}\n"));
    }
    write("Cargo.lock", "# north-base gains a dependency\n");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "Move the worker sandbox's path policy into north-base"]);
    (base, git(repo, &["rev-parse", "HEAD"]))
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn harness_data_repo(repo: &Path) -> (String, String) {
    for table in [STRUCTURE, TABLE, tree_wide_units::TABLE, super::super::guard_suites::TABLE, ".safe-push"] {
        std::fs::create_dir_all(repo.join(table).parent().unwrap()).unwrap();
        std::fs::copy(root().join(table), repo.join(table)).unwrap();
    }
    copy_tree(&root().join("tests/it"), &repo.join("tests/it"));
    std::fs::create_dir_all(repo.join("harness")).unwrap();
    std::fs::write(repo.join("harness/defaults.toml"), "[defaults]\nvalue = 1\n").unwrap();
    git(repo, &["init", "-q"]);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "base"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("harness/defaults.toml"), "[defaults]\nvalue = 2\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "Change only harness data"]);
    (base, git(repo, &["rev-parse", "HEAD"]))
}

#[test]
fn a_lane_changing_only_harness_data_runs_the_tests_that_read_it() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let (base, head) = harness_data_repo(&repo);
    let commands = super::super::select::Config::load(&repo, &head).commands(&repo, &base, &head).unwrap();
    assert!(commands.iter().any(|command| command.contains("SAFE_PUSH_LAND_IT")), "the Rust test check fires for a harness-data-only lane: {commands:?}");
    let answer = r#"{"tests":["north-fleet fleet::supervisor::tests::reads_defaults"],"always":[],"added":[],"files":[],"untraced":null,"known":1}"#;
    let graph = graph_stub(directory.path(), "graph", &format!("printf '%s' '{answer}'"));
    let selection = selection_asking_with(&graph, &repo, &base, &head, &Batch::default(), BTreeMap::new).unwrap();
    assert!(selection.arguments().contains("(binary_id(north-fleet)&(test(=fleet::supervisor::tests::reads_defaults)))"), "the test that reads harness/defaults.toml runs: {}", selection.arguments());
    assert!(selection.packages().contains("north-fleet"), "the package of that test is built: {:?}", selection.packages());

    let traced = super::super::graph_selection::Tests::parse(r#"{"tests":["north-a a::whole"],"added":[{"package":"north-a","name":"a::unknown"}],"known":2}"#).unwrap();
    let listed = BTreeMap::from([
        ("north-a".to_owned(), super::super::graph_selection::Listed { package: "north-a".to_owned(), tests: ["a::whole".to_owned(), "a::unknown".to_owned()].into() }),
        ("north-b::it".to_owned(), super::super::graph_selection::Listed { package: "north-b".to_owned(), tests: ["b::one".to_owned(), "b::two".to_owned()].into() }),
    ]);
    let collapsed = traced.with_listed(listed).unwrap().arguments();
    assert!(collapsed.contains("package(north-a)") && !collapsed.contains("a::whole"), "a wholly included package is one term, and the listing finds its unknown test: {collapsed}");
    assert!(!collapsed.contains("north-b"), "a package with no selected test is not run: {collapsed}");
    let many: Vec<String> = (0..8_000).map(|index| format!("north-a a::test_{index:05}")).collect();
    let answer = format!(r#"{{"tests":["{}"],"known":8000}}"#, many.join("\",\""));
    let huge = super::super::graph_selection::Tests::parse(&answer).unwrap();
    assert!(huge.with_listed(BTreeMap::new()).is_none(), "a filter past the limit falls back to the whole closure instead of dropping tests");
}

#[test]
fn a_moved_module_selects_the_tests_that_reach_it() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir_all(repo.join("tests/it")).unwrap();
    let (base, head) = replay_of_the_sandbox_move(&repo);
    let answer = r#"{"tests":["north-runtime worker_sandbox::tests::lane_only_reads_the_lane","north-runtime worker_sandbox::broker::tests::answers","north-providers providers::opencode::turns::tests::a_turn_runs","north-providers providers::opencode::harness::tests::wraps"],"always":[],"added":[],"files":[],"untraced":null,"known":1151}"#;
    let graph = graph_stub(directory.path(), "graph", &format!("printf '%s' '{answer}'"));
    let selection = selection_asking_with(&graph, &repo, &base, &head, &Batch::default(), BTreeMap::new).unwrap();
    let arguments = selection.arguments();
    assert!(selection.runs_module("worker_sandbox"), "the tests/it module that spawns north and holds {SANDBOX_TEST}, which the graph does not know, runs because the lane changes crates its binary links: {arguments}");
    assert!(arguments.contains("worker_sandbox|") || arguments.contains("|worker_sandbox)") || arguments.contains("(worker_sandbox)"), "the filter names the module: {arguments}");
    assert!(arguments.contains("(package(north-runtime)&test(/^worker_sandbox::/))") && arguments.contains("package(north-land-sandbox)"), "the worker sandbox guard suite runs: {arguments}");
    assert!(selection.packages().is_superset(&["north".to_owned(), "north-runtime".to_owned(), "north-land-sandbox".to_owned()].into()), "the packages of the module and the suite are built");
    let mut refusal = Vec::new();
    assert!(super::super::guard_suites::refuse(&repo, &base, &head, &mut refusal), "moving guarded files out of the suite's paths refuses the lane");
    let refusal = String::from_utf8(refusal).unwrap();
    assert!(refusal.contains("crates/north-runtime/src/worker_sandbox/lane_only.rs moved to crates/north-base/src/platform/worker_sandbox/lane_only.rs"), "{refusal}");
}
