use std::path::Path;
use std::process::{Command, Stdio};

use super::Buck2;
use crate::cargo::Cargo;
use crate::BuildSystem;

/// The lint line, the Rust test line and one scan line of north's `.safe-push` on 2026-11-12.
const CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets ${SAFE_PUSH_LAND_CHANGED:---workspace} --locked --offline -- -D warnings";
const NEXTEST: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail ${SAFE_PUSH_LAND_PACKAGES:---workspace} --lib --bins ${SAFE_PUSH_LAND_IT:---test it -E kind(lib)|kind(bin)|kind(test)}";
/// The Buck2 clippy gate the lint line becomes where the gate takes it.
const GATE: &str = "./bin/land-shell --dev -- buck2 bxl --remote-only --materializations=none //lint/clippy.bxl:clippy -- --targets //...";
const SCAN: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-buck/src/,crates/north-build-system/src/,src/cli/ --no-build buck targets --check";

#[test]
fn a_buck2_host_runs_its_own_tests_only_behind_the_test_switch_and_moves_a_lint_line_only_through_the_guarded_gate() {
    let (builds, tests_too) = (Buck2 { tests: false }, Buck2 { tests: true });

    assert_eq!((builds.land_line(CLIPPY), tests_too.land_line(CLIPPY)), (CLIPPY.to_owned(), CLIPPY.to_owned()), "a lint line is not rewritten where the checks are listed: only the gate, behind buck2_lint and the base's gate and configuration, moves it");
    assert_eq!(builds.land_line(NEXTEST), NEXTEST, "the test run stays Cargo's until the host sets buck2_tests");
    assert_eq!(tests_too.land_line(NEXTEST), "./bin/land-shell --dev -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re //...");
    assert_eq!((tests_too.land_line(SCAN), Cargo.land_line(CLIPPY)), (SCAN.to_owned(), CLIPPY.to_owned()), "a line that runs no lint or test, and every line on a Cargo host, runs as declared");
    assert!(crate::lints(CLIPPY) && crate::lints(GATE) && !crate::lints(NEXTEST), "the lint line of either side is told from a test line");
}

#[test]
fn a_buck2_host_builds_a_tree_s_north_with_buck2_and_calls_the_tree_broken_only_on_buck2_s_user_error() {
    let side = Buck2 { tests: false };
    let (invocation, binary) = side.tree_binary(Path::new("/tree"), "north");

    assert_eq!(invocation.words, ["buck2", "build", "--remote-only", "//:north", "--out", "/tree/buck-out/north/dev/north", "--no-remote-cache"]);
    assert_eq!(binary, Path::new("/tree/buck-out/north/dev/north"));
    assert!(side.broken(Some(3), "") && !side.broken(Some(2), "") && !side.broken(None, ""), "only exit 3, Buck2's user error, blames the tree; an engine or daemon failure gives no verdict");
}

#[test]
fn a_lint_only_host_runs_the_gate_only_while_the_tree_s_gate_script_and_buck2_configuration_are_the_base_s() {
    let tree = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(tree.path()).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).stdout(Stdio::null()).status().unwrap().success());
    let write = |name: &str, text: &str| std::fs::write(tree.path().join(name), text).unwrap();
    std::fs::create_dir(tree.path().join("lint")).unwrap();
    for (name, text) in [(".buckconfig", "[cells]\n  root = .\n"), ("lint/clippy.bxl", "gate\n"), ("code.rs", "one\n")] {
        write(name, text);
    }
    for arguments in [&["init", "-q"][..], &["add", "."], &["commit", "-qm", "base"], &["tag", "base"]] {
        git(arguments);
    }
    let gate = |declared: &str| super::gate(tree.path(), "base", declared);

    write("code.rs", "two\n");
    assert_eq!(
        gate(CLIPPY).as_deref(),
        Some(GATE),
        "a lane that changes code alone is linted by the gate"
    );
    assert_eq!((gate(NEXTEST), gate(SCAN)), (None, None), "the key moves the lint line alone");
    assert_eq!(super::gate(tree.path(), "no-such-base", CLIPPY), None, "a base this checkout lacks leaves the line as declared");
    write("lint/clippy.bxl", "a lane's own gate\n");
    assert_eq!(gate(CLIPPY), None, "a lane that rewrites the gate script is linted by Cargo, not by its own gate");
    write("lint/clippy.bxl", "gate\n");
    write(".buckconfig", "[buck2_re_client]\n  engine_address = grpc://lane:1\n");
    assert_eq!(gate(CLIPPY), None, "a lane whose .buckconfig names its own endpoint is linted by Cargo, since the tree's file outranks the host's");
    git(&["checkout", "-q", "--", ".buckconfig"]);
    write(".buckconfig.local", "[buck2_re_client]\n  engine_address = grpc://lane:1\n");
    assert_eq!(gate(CLIPPY), None, "and so is a tree with an untracked buckconfig file");
    std::fs::remove_file(tree.path().join(".buckconfig.local")).unwrap();
    assert!(gate(CLIPPY).is_some(), "the gate runs again once the tree's own configuration is the base's");

    let toolchain = tree.path().join(".buckconfig.flake");
    let link = |target: &Path| {
        let _ = std::fs::remove_file(&toolchain);
        std::os::unix::fs::symlink(target, &toolchain).unwrap();
    };
    write(".buckconfig.flake", "[buck2_re_client]\n  engine_address = grpc://lane:1\n");
    assert_eq!(gate(CLIPPY), None, "a plain file where the shell hook's link belongs is linted by Cargo: the tree's .buckconfig includes it, any section");
    git(&["add", "--force", ".buckconfig.flake"]);
    let stored = std::fs::read_dir("/nix/store").unwrap().next().unwrap().unwrap().path();
    link(&stored);
    assert_eq!(gate(CLIPPY), None, "and so is a toolchain buckconfig the tree tracks, whatever stands in the checkout");
    git(&["rm", "-q", "--cached", "-f", ".buckconfig.flake"]);
    assert!(gate(CLIPPY).is_some(), "the untracked link into the store that the hook makes is the host's own");
    link(&tree.path().join("code.rs"));
    assert_eq!(gate(CLIPPY), None, "a link to anything outside the store is not the hook's");
    link(Path::new(&format!("/nix/store/../..{}", tree.path().join("code.rs").display())));
    assert_eq!(gate(CLIPPY), None, "nor is one that starts with the store's path and reaches a file of the tree");
    link(Path::new("/nix/store/no-such-toolchain.buckconfig"));
    assert_eq!(gate(CLIPPY), None, "nor one that reaches nothing");
}

#[test]
fn a_host_that_lists_crates_tests_them_with_buck2_and_leaves_the_rest_of_the_selection_to_the_declared_line() {
    let tree = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(tree.path()).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).stdout(Stdio::null()).status().unwrap().success());
    for name in ["crates/a", "crates/b", "crates/c", "crates/x/m"] {
        std::fs::create_dir_all(tree.path().join(name)).unwrap();
    }
    for (name, text) in [(".buckconfig", "[cells]\n  root = .\n"), ("BUCK", "north\n"), ("crates/a/BUCK", "a\n"), ("crates/b/BUCK", "b\n"), ("crates/c/lib.rs", "c\n"), ("crates/x/m/BUCK", "m\n")] {
        std::fs::write(tree.path().join(name), text).unwrap();
    }
    for arguments in [&["init", "-q"][..], &["add", "."], &["commit", "-qm", "base"], &["tag", "base"]] {
        git(arguments);
    }
    let listed = ["a", "c", "gone", "m", "north"].map(str::to_owned).into_iter().collect();
    let packages = [("a", "//crates/a:"), ("b", "//crates/b:"), ("c", "//crates/c:"), ("m", "//crates/x/m:"), ("north", "//:")].map(|(name, package)| (name.to_owned(), package.to_owned())).into();
    let none = std::collections::BTreeSet::new();
    let passed: std::collections::BTreeSet<String> = ["a".to_owned()].into();
    let split = |declared: &str, selected: &str| super::crate_tests(tree.path(), "base", declared, ("SAFE_PUSH_LAND_PACKAGES", selected), &listed, &packages).map(|split| ((split.moved.clone(), split.line.clone()), split.left(&passed), split.left(&none)));
    let on_buck2 = |names: &[&str], targets: &str| (names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>(), format!("./bin/land-shell --dev -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re {targets}"));
    let flags = |flags: &str| Some(flags.to_owned());

    assert_eq!(split(NEXTEST, "-p a -p b"), Some((on_buck2(&["a"], "//crates/a:"), flags("-p b"), flags("-p a -p b"))), "the listed crate moves, and stays on the declared line unless Buck2 passed it");
    assert_eq!(split(NEXTEST, "-p a"), Some((on_buck2(&["a"], "//crates/a:"), None, flags("-p a"))), "nothing is left for the declared line once Buck2 passed every selected package");
    assert_eq!(
        split(NEXTEST, "--workspace"),
        Some((on_buck2(&["a", "m", "north"], "//crates/a: //crates/x/m: //:"), flags("--workspace --exclude a"), flags("--workspace"))),
        "a workspace run moves what is listed and has targets, the root package and one whose manifest sits deeper too, in one Buck2 run: not the crate with no BUCK file nor the one with no package (north#1299)"
    );
    assert_eq!(split(NEXTEST, "-p b -p c"), None, "a selection with nothing to move runs as declared");
    let guards = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail -p a -p b --lib -E '(binary_id(=a) & test(=t)) | (binary_id(=b) & test(=u))'";
    let (reading, named) = super::named_selection(guards, "SAFE_PUSH_LAND_PACKAGES").unwrap();
    assert_eq!((reading.as_str(), named.as_str()), ("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail ${SAFE_PUSH_LAND_PACKAGES} --lib -E '(binary_id(=a) & test(=t)) | (binary_id(=b) & test(=u))'", "-p a -p b"), "a line that names its own packages reads them from the variable instead, keeping its filter");
    assert_eq!(split(&reading, &named), Some((on_buck2(&["a"], "//crates/a:"), flags("-p b"), flags("-p a -p b"))), "and is split over the packages it named, as the line that reads the selection is");
    assert!(super::selects_packages(guards, "SAFE_PUSH_LAND_PACKAGES") && !super::selects_packages(CLIPPY, "SAFE_PUSH_LAND_PACKAGES"), "the runner hands such a line to the split");
    assert_eq!((super::named_selection(NEXTEST, "SAFE_PUSH_LAND_PACKAGES"), super::named_selection(&format!("{guards} -p c"), "SAFE_PUSH_LAND_PACKAGES")), (None, None), "a line that reads the variable, or names its packages apart, keeps its own form");
    assert_eq!(split(CLIPPY, "-p a"), None, "the key moves the nextest line alone");
    let moved = super::crate_tests(tree.path(), "base", NEXTEST, ("SAFE_PUSH_LAND_PACKAGES", "--workspace"), &listed, &packages).unwrap();
    let owners = ["root//crates/a:a-unittest", "root//crates/ab:ab-unittest", "root//:north-unittest", "root//crates/b:b-unittest", "root//crates/x/m:m-unittest", "root//crates/m:m-unittest"].map(|label| moved.owner(label));
    assert_eq!(owners, [Some("a"), None, Some("north"), None, Some("m"), None], "a target in Buck2's record belongs to the moved crate whose package holds it");
    std::fs::write(tree.path().join("crates/a/BUCK"), "a test that passes whatever the code does\n").unwrap();
    assert_eq!(split(NEXTEST, "-p a"), None, "a lane that changes a BUCK file is tested by Cargo: it could define its own passing test");
    let builds_with_buck2 = || super::tree_builder(tree.path(), "base").is_some();
    assert!(!builds_with_buck2(), "and its own north is built by Cargo, since it could define the north its check runs");
    git(&["checkout", "-q", "--", "crates/a/BUCK"]);
    std::fs::write(tree.path().join("crates/b/rules.bzl"), "a rule\n").unwrap();
    assert_eq!(split(NEXTEST, "-p a"), None, "and so is one that adds a rule file anywhere");
    std::fs::remove_file(tree.path().join("crates/b/rules.bzl")).unwrap();
    std::fs::write(tree.path().join("crates/a/PACKAGE"), "a value every target of the package reads\n").unwrap();
    assert_eq!(split(NEXTEST, "-p a"), None, "or a PACKAGE file");
    std::fs::remove_file(tree.path().join("crates/a/PACKAGE")).unwrap();
    assert!(split(NEXTEST, "-p a").is_some(), "the split is issued again once the target definitions are the base's");
    assert!(builds_with_buck2(), "and the tree's north is built by Buck2 again");
    std::fs::write(tree.path().join(".buckconfig"), "[buck2_re_client]\n  engine_address = grpc://lane:1\n").unwrap();
    assert_eq!(split(NEXTEST, "-p a"), None, "a lane whose .buckconfig names its own endpoint is tested by Cargo");
    assert!(!builds_with_buck2(), "and its north is built by Cargo");
}
