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
