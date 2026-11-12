use std::path::Path;
use std::process::{Command, Stdio};

use super::Buck2;
use crate::cargo::Cargo;
use crate::BuildSystem;

/// The lint line, the Rust test line and one scan line of north's `.safe-push` on 2026-11-12.
const CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets ${SAFE_PUSH_LAND_CHANGED:---workspace} --locked --offline -- -D warnings";
const NEXTEST: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail ${SAFE_PUSH_LAND_PACKAGES:---workspace} --lib --bins ${SAFE_PUSH_LAND_IT:---test it -E kind(lib)|kind(bin)|kind(test)}";
const SCAN: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-buck/src/,crates/north-build-system/src/,src/cli/ --no-build buck targets --check";

#[test]
fn a_buck2_host_lints_with_its_own_gate_and_runs_its_own_tests_only_behind_the_test_switch() {
    let (builds, tests_too) = (Buck2 { tests: false }, Buck2 { tests: true });

    assert_eq!(builds.land_line(CLIPPY), "./bin/land-shell --dev -- buck2 bxl --remote-only --materializations=none //lint/clippy.bxl:clippy -- --targets //...");
    assert_eq!(builds.land_line(NEXTEST), NEXTEST, "the test run stays Cargo's until the host sets buck2_tests");
    assert_eq!(tests_too.land_line(NEXTEST), "./bin/land-shell --dev -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re //...");
    assert_eq!((tests_too.land_line(SCAN), Cargo.land_line(CLIPPY)), (SCAN.to_owned(), CLIPPY.to_owned()), "a line that runs no lint or test, and every line on a Cargo host, runs as declared");
    assert!(crate::lints(CLIPPY) && crate::lints(&builds.land_line(CLIPPY)) && !crate::lints(NEXTEST), "the lint line of either side is told from a test line");
}

#[test]
fn a_buck2_host_builds_a_tree_s_north_with_buck2_and_calls_the_tree_broken_only_on_buck2_s_user_error() {
    let side = Buck2 { tests: false };
    let (invocation, binary) = side.tree_binary(Path::new("/tree"), "north");

    assert_eq!(invocation.words, ["buck2", "build", "--remote-only", "//:north", "--out", "/tree/buck-out/north/dev/"]);
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
        Some("./bin/land-shell --dev -- buck2 bxl --remote-only --materializations=none //lint/clippy.bxl:clippy -- --targets //..."),
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
    link(Path::new("/nix/store/toolchain.buckconfig"));
    assert_eq!(gate(CLIPPY), None, "and so is a toolchain buckconfig the tree tracks, whatever stands in the checkout");
    git(&["rm", "-q", "--cached", "-f", ".buckconfig.flake"]);
    assert!(gate(CLIPPY).is_some(), "the untracked link into the store that the hook makes is the host's own");
    link(&tree.path().join("code.rs"));
    assert_eq!(gate(CLIPPY), None, "a link to anything outside the store is not the hook's");
}
