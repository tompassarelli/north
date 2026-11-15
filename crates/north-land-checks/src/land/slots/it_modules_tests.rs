use north_foundation::test_inputs::git::git_in as git;
use super::*;
use super::super::tree_wide_units;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn graph_stub(directory: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = directory.join(name);
    north_foundation::foundation::shim::write_script(&path, &format!("{body}\n"));
    path
}

fn listing(ids: &[&str]) -> Option<super::super::graph_selection::ListedTests> {
    let mut listed: super::super::graph_selection::ListedTests = BTreeMap::new();
    for id in ids {
        let (binary, test) = id.split_once(' ').unwrap();
        let suite = listed.entry(binary.to_owned()).or_insert_with(|| super::super::graph_selection::Listed { package: binary.split("::").next().unwrap().to_owned(), tests: ["unselected::neighbour".to_owned()].into() });
        suite.tests.insert(test.to_owned());
    }
    Some(listed)
}

/// A file's content without its mode: a remote worker's inputs are read-only, and these fixtures copy one file twice and edit their copies.
fn copy(from: &Path, to: &Path) {
    std::fs::write(to, std::fs::read(from).unwrap()).unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            copy(&entry.path(), &target);
        }
    }
}

fn harness_data_repo(repo: &Path) -> (String, String) {
    for table in [STRUCTURE, TABLE, tree_wide_units::TABLE, super::super::guard_suites::TABLE, ".safe-push"] {
        std::fs::create_dir_all(repo.join(table).parent().unwrap()).unwrap();
        copy(&root().join(table), &repo.join(table));
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
    let selection = selection_asking_with(&graph, &repo, &base, &head, Stage::BeforePush, |_| listing(&["north-fleet fleet::supervisor::tests::reads_defaults"])).unwrap();
    a_change_only_inside_a_separate_workspace_runs_no_test_before_the_push_when_the_graph_reaches_none();
    assert!(selection.arguments().contains("(binary_id(north-fleet)&(test(=fleet::supervisor::tests::reads_defaults)))"), "the test that reads harness/defaults.toml runs: {}", selection.arguments());
    assert!(selection.packages().contains("north-fleet"), "the package of that test is built: {:?}", selection.packages());

    let traced = super::super::graph_selection::Tests::parse(r#"{"tests":["north-a a::whole","north-gone gone::test"],"added":[{"package":"north-a","name":"a::unknown"}],"known":2}"#).unwrap();
    let listed = BTreeMap::from([
        ("north-a".to_owned(), super::super::graph_selection::Listed { package: "north-a".to_owned(), tests: ["a::whole".to_owned(), "a::unknown".to_owned()].into() }),
        ("north-b::it".to_owned(), super::super::graph_selection::Listed { package: "north-b".to_owned(), tests: ["b::one".to_owned(), "b::two".to_owned()].into() }),
    ]);
    let collapsed_tests = traced.with_listed(Some(listed)).unwrap();
    assert_eq!(collapsed_tests.unknown_share(), Some((1, 4)), "one unknown test of the four the build lists, which is over the tenth that starts a refresh");
    let collapsed = collapsed_tests.arguments();
    assert!(collapsed.contains("package(north-a)") && !collapsed.contains("a::whole"), "a wholly included package is one term, and the listing finds its unknown test: {collapsed}");
    assert!(!collapsed.contains("north-gone"), "a binary the listing no longer has, such as a retired crate's, is not named: {collapsed}");
    assert!(!collapsed.contains("north-b"), "a package with no selected test is not run: {collapsed}");
    let suite = |package: &str| super::super::graph_selection::Listed { package: package.to_owned(), tests: ["t".to_owned()].into() };
    let held = BTreeMap::from([("north-lander".to_owned(), suite("north-lander")), ("north-lander::it".to_owned(), suite("north-lander")), ("north-lander-doctor".to_owned(), suite("north-lander-doctor"))]);
    let asked = super::super::graph_selection::Scope::Packages(["north-lander".to_owned()].into());
    let cut = super::super::graph_selection::within(held.clone(), &asked);
    assert_eq!(cut.keys().map(String::as_str).collect::<Vec<_>>(), ["north-lander", "north-lander::it"], "a listing over the test check's wider packages hands the selection only the suites of the packages it asked for, by each suite's package, not a name prefix");
    assert_eq!(super::super::graph_selection::within(held.clone(), &super::super::graph_selection::Scope::Workspace), held, "a workspace listing is not cut");
    let unlisted = super::super::graph_selection::Tests::parse(r#"{"tests":["north-gone gone::test"],"known":1}"#).unwrap();
    assert!(unlisted.with_listed(None).is_none(), "with nothing listed, a binary the graph names cannot be checked, so the selection falls back to the crate closure instead of a filter nextest refuses (north#1099)");
    let many: Vec<String> = (0..8_000).map(|index| format!("north-a a::test_{index:05}")).collect();
    let answer = format!(r#"{{"tests":["{}"],"known":8000}}"#, many.join("\",\""));
    let huge = super::super::graph_selection::Tests::parse(&answer).unwrap();
    assert!(huge.with_listed(None).is_none(), "a filter past the limit falls back to the whole closure instead of dropping tests");
}

fn a_change_only_inside_a_separate_workspace_runs_no_test_before_the_push_when_the_graph_reaches_none() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir_all(repo.join("evals/b/src")).unwrap();
    harness_data_repo(&repo);
    let write = |path: &str, text: &str| std::fs::write(repo.join(path), text).unwrap();
    write("evals/b/Cargo.toml", "[package]\nname = \"eval-b\"\nversion = \"0.1.0\"\n\n[workspace]\n");
    write("evals/b/src/main.rs", "fn main() {}\n");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "Add a runner with its own workspace"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    write("evals/b/src/main.rs", "fn main() {\n    println!(\"eval\");\n}\n");
    git(&repo, &["commit", "-qam", "Change only the runner"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let answer = r#"{"tests":[],"always":[],"added":[],"files":["evals/b/src/main.rs"],"untraced":null,"known":2030}"#;
    let graph = graph_stub(directory.path(), "graph", &format!("printf '%s' '{answer}'"));
    let selection = selection_asking_with(&graph, &repo, &base, &head, Stage::BeforePush, |_| None).unwrap();
    assert!(matches!(selection, Selection::NoTest(_)), "an evals/b-only lane the graph reaches no test from runs none before its ticket, not every test of the workspace (north#1039): {}", selection.summary());
}
