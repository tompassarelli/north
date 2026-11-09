#![cfg(test)]

use super::gate::{Class, class_of};

#[test]
fn the_gate_skips_only_an_invocation_it_already_executed_in_the_same_process() {
    assert_eq!(super::gate::executed("7@pid:[1]", Some("7@pid:[1]\ta /a/bun\tb /b/bun")), vec!["a /a/bun".to_owned(), "b /b/bun".to_owned()]);
    assert!(super::gate::executed("8@pid:[1]", Some("7@pid:[1]\ta /a/bun")).is_empty(), "a child process inherited its parent's exec chain");
    assert!(super::gate::executed("7@pid:[2]", Some("7@pid:[1]\ta /a/bun")).is_empty(), "a process in a nested pid namespace inherited a chain from another namespace's same pid");
    assert!(super::gate::executed("7@pid:[1]", None).is_empty());
    let nix = std::path::Path::new("/nonexistent/nix");
    let outer = super::gate::invocation(nix, &["develop".to_owned(), "-c".to_owned(), "bash".to_owned(), "check.sh".to_owned()]);
    let inner = super::gate::invocation(nix, &["develop".to_owned(), "-c".to_owned(), "cargo".to_owned(), "run".to_owned()]);
    assert_ne!(outer, inner, "a nested nix develop with other arguments counted as a loop");
    assert_eq!(super::gate::executed_value("7@pid:[1]", std::slice::from_ref(&outer), nix, &["develop".to_owned()]), format!("7@pid:[1]\t{outer}\t{}", super::gate::invocation(nix, &["develop".to_owned()])));
}

#[test]
fn heavy_command_fixture_rows_take_their_gate_class_through_nix_and_shell_wrappers() {
    let rows: Vec<(String, String, String)> = serde_json::from_str(include_str!("../../../north-guard/src/guard/fixtures/heavy_command.json")).unwrap();
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let mut wrong = Vec::new();
    for (command, _hook, expected) in &rows {
        for argv in [
            vec!["bash".to_owned(), "-c".to_owned(), command.clone()],
            vec!["bash".to_owned(), "-c".to_owned(), format!("echo ready && {command}")],
            crate::shell::words(&format!("env FLAG=1 nix -L develop -c bash -lc {}", quote(command))).unwrap(),
        ] {
            let observed = class_of(&argv, 0).map_or("none", |class| match class {
                Class::Heavy => "heavy",
                Class::Moderate => "moderate",
            });
            if observed != expected {
                wrong.push(format!("{argv:?}: {observed}, fixture {expected}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_gate_classes_a_direct_rustc_compile_and_nextest_but_not_a_rustc_probe() {
    let class = |text: &str| class_of(&crate::shell::words(text).unwrap(), 0);
    assert_eq!(class("rustc src/main.rs -o main"), Some(Class::Heavy));
    assert_eq!(class("rustc -vV"), None);
    assert_eq!(class("rustc - --crate-name ___ --print=file-names"), None);
    assert_eq!(class("cargo-nextest nextest run"), Some(Class::Heavy));
    assert_eq!(class("cargo metadata"), None);
}

#[test]
fn a_fleet_sessions_cargo_fix_help_and_version_are_not_offloaded() {
    let words = |text: &str| crate::shell::words(text).unwrap();
    for stays in ["cargo fix", "cargo clippy --fix", "cargo build --help", "cargo-nextest --version", "cargo test -h"] {
        assert!(!super::gate::agent_compile_in(&words(stays), true), "{stays}");
    }
    for goes in ["cargo build", "cargo test fix", "cargo nextest run", "rustc a.rs", "cargo-nextest nextest run", "depbox run -- nix develop -c cargo test --locked", "bash -c 'cd x && rustc a.rs'", "nix develop -c cargo-nextest nextest run"] {
        assert!(super::gate::agent_compile_in(&words(goes), true), "{goes}");
    }
    assert!(!super::gate::agent_compile_in(&words("cargo build"), false), "Tom's own shell");
    for stays in ["bun test", "depbox run -- bun test", "bash -c 'cargo fix'", "depbox fetch -- cargo fetch"] {
        assert!(!super::gate::agent_compile_in(&words(stays), true), "{stays}");
    }
}

#[test]
fn a_build_that_writes_a_file_the_caller_runs_is_an_artifact_build_and_checks_tests_and_runs_are_not() {
    let words = |text: &str| crate::shell::words(text).unwrap();
    for artifact in ["cargo build --release -p north-frontier", "cargo b --locked", "cargo +nightly build -p north-frontier --bin runner", "cargo install --path crates/x", "cargo rustc --release", "rustc src/main.rs -o main", "depbox run -- nix develop -c cargo build --release -p north-frontier", "bash -c 'cd x && cargo build'", "bun build src/a.ts --outdir dist", "bun build a.ts --compile --outfile=a", "cargo test -q --no-run --locked --offline -p north --test it", "depbox run -- north capacity dev-shell run -- env GIT_CONFIG_NOSYSTEM=1 cargo test -q --no-run --locked --offline -p north --test it"] {
        assert!(super::gate::builds_artifact(&words(artifact)), "{artifact}");
    }
    for offloadable in ["cargo check", "cargo clippy --all-targets", "cargo test", "cargo nextest run", "cargo run -p north-frontier", "cargo doc", "cargo build --help", "cargo fix", "rustc -vV", "cargo-nextest nextest run", "bun build a.ts", "bun test"] {
        assert!(!super::gate::builds_artifact(&words(offloadable)), "{offloadable}");
    }
}

#[test]
fn a_cargo_build_inside_depbox_through_north_dev_shell_run_is_classed_so_depbox_takes_its_lease() {
    let words = |text: &str| crate::shell::words(text).unwrap();
    let build = words("depbox run -- north capacity dev-shell run -- env -u GIT_AUTHOR_NAME GIT_CONFIG_GLOBAL=/dev/null cargo test -q --no-run --locked --offline -p north --test it");
    assert_eq!(class_of(&build, 0), Some(Class::Heavy));
    assert!(super::gate::agent_compile_in(&build, true));
    assert_eq!(class_of(&words("depbox run -- north capacity dev-shell run -- cargo check"), 0), Some(Class::Moderate));
    assert_eq!(class_of(&words("depbox run -- north capacity dev-shell run -- git status"), 0), None);
}
