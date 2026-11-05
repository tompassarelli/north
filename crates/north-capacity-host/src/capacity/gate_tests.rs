#![cfg(test)]

use super::gate::{Class, class_of, inside_lease};

fn class(line: &str) -> Option<Class> {
    class_of(&crate::shell::words(line).unwrap(), 0)
}

#[test]
fn cargo_builds_and_tests_take_a_heavy_lease_and_checks_a_moderate_one() {
    assert_eq!(class("cargo test --locked --offline"), Some(Class::Heavy));
    assert_eq!(class("cargo +nightly -C crates/x build --release"), Some(Class::Heavy));
    assert_eq!(class("cargo --config build.jobs=2 clippy --all-targets"), Some(Class::Moderate));
    assert_eq!(class("cargo fmt --check"), None);
    assert_eq!(class("cargo metadata --format-version 1"), None);
    assert_eq!(class("cargo --version"), None);
}

#[test]
fn bun_builds_suites_and_map_builds_take_a_moderate_lease_and_one_test_file_or_a_script_none() {
    assert_eq!(class("bun build ./src/index.ts --outdir dist"), Some(Class::Moderate));
    assert_eq!(class("bun test"), Some(Class::Moderate));
    assert_eq!(class("bun test host/lan"), Some(Class::Moderate));
    assert_eq!(class("bun test host/lan/pool.test.ts"), None);
    assert_eq!(class("bun wisp map build --profile classic"), Some(Class::Moderate));
    assert_eq!(class("bun run wisp map build"), Some(Class::Moderate));
    assert_eq!(class("bun scripts/wisp.ts map build"), Some(Class::Moderate));
    assert_eq!(class("bun scripts/wisp.ts lan pool --pair 6 --queue"), None);
    assert_eq!(class("bun harness/skills/machine-capacity/scripts/machine-capacity.mjs run --class heavy -- bun wisp map build"), None);
    assert_eq!(class("north capacity host run --class heavy -- bun wisp map build"), None);
}

#[test]
fn a_wrapped_build_is_seen_through_depbox_nix_develop_env_and_a_shell_script() {
    assert_eq!(class("depbox run -- nix develop -c cargo test --locked"), Some(Class::Heavy));
    assert_eq!(class("depbox run -- nix develop -c sh -c 'cargo fmt --check && cargo clippy -- -D warnings'"), Some(Class::Moderate));
    assert_eq!(class("depbox run -- bash -c 'bun test >/tmp/log 2>&1; cargo build'"), Some(Class::Heavy));
    assert_eq!(class("depbox run -- env CARGO_INCREMENTAL=0 nice -n 19 cargo build"), Some(Class::Heavy));
    assert_eq!(class("depbox fetch -- bun install"), None);
    assert_eq!(class("depbox run -- git status"), None);
}

#[test]
fn a_capacity_scope_or_a_private_desktop_counts_as_inside_a_lease_and_a_session_scope_does_not() {
    assert!(inside_lease("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity-0123456789abcdef0123456789abcdef.scope\n"));
    assert!(inside_lease("0::/user.slice/user-1000.slice/user@1000.service/app.slice/private-desktop-client-870196.scope\n"));
    assert!(!inside_lease("0::/user.slice/user-1000.slice/session-39733.scope\n"));
    assert!(!inside_lease("0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-fleet-gex45-disk-94360c08.service\n"));
}

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
fn a_leased_nix_build_takes_the_lease_cores_and_one_job_unless_it_sets_its_own() {
    let sized = |line: &str| north_base::nix_command::sized(&crate::shell::words(line).unwrap(), "6").map(|words| words.join(" "));
    assert_eq!(sized("nix build .#shellcheck --rebuild").as_deref(), Some("nix --cores 6 --max-jobs 1 build .#shellcheck --rebuild"));
    assert_eq!(sized("/nix/store/x-nix/bin/nix-build default.nix").as_deref(), Some("/nix/store/x-nix/bin/nix-build --cores 6 --max-jobs 1 default.nix"));
    assert_eq!(sized("nix flake check").as_deref(), Some("nix --cores 6 --max-jobs 1 flake check"));
    assert_eq!(sized("nix build -j4 .#x"), None);
    assert_eq!(sized("nix --cores 2 build .#x"), None);
    assert_eq!(sized("nix develop -c cargo build"), None);
    assert_eq!(sized("nix eval .#x"), None);
    assert_eq!(sized("cargo build"), None);
}
