use super::*;
use crate::nix_gc::tests::{Fake, home, wrapper};

const FIRN_23_PARENT: &str = include_str!("fixtures/firn-23-parent.lock");

fn lines(output: Result<Vec<String>, String>) -> String {
    output.unwrap().join("\n")
}

#[test]
fn doctor_nix_lock_check_refuses_firn_23_parent_lock_naming_the_transitive_nixpkgs_node() {
    let refused = lines(lock_refusals(FIRN_23_PARENT, None));
    assert!(refused.contains("transitive nixpkgs node `nixpkgs` (nixos-unstable cf3f5c4d), pulled in by elephant"), "{refused}");
    assert!(!refused.contains("`nixpkgs_2`"), "the root's own nixpkgs is the flake's: {refused}");
}

#[test]
fn doctor_nix_lock_check_passes_a_listed_node_and_refuses_a_listed_node_whose_rev_moved() {
    let allow = |rev: &str| format!("[[allow]]\nnode = \"nixpkgs\"\nrev = \"{rev}\"\nreason = \"elephant pins its own nixpkgs\"\n");
    let seen = lines(lock_refusals(FIRN_23_PARENT, Some(&allow("cf3f5c4def3c7b5f1fc012b3d839575dbe552d43"))));
    assert!(!seen.contains("`nixpkgs` "), "{seen}");
    let moved = lines(lock_refusals(FIRN_23_PARENT, Some(&allow("0000"))));
    assert!(moved.contains("`nixpkgs` "), "{moved}");
}

#[test]
fn doctor_nix_allowlist_entry_without_a_reason_is_an_error() {
    assert!(lock_refusals(FIRN_23_PARENT, Some("[[allow]]\nnode = \"nixpkgs\"\n")).is_err());
    assert!(lock_refusals(FIRN_23_PARENT, Some("[[allow]]\nnode = \"nixpkgs\"\nreason = \" \"\n")).is_err());
}

#[test]
fn doctor_nix_lock_check_passes_norths_own_lock() {
    assert_eq!(lines(lock_refusals(include_str!("../../../../flake.lock"), None)), "");
}

#[test]
fn doctor_nix_lock_command_exits_1_on_the_refused_lock_and_0_on_a_clean_one() {
    let dir = tempfile::tempdir().unwrap();
    let firn = dir.path().join("firn.lock");
    std::fs::write(&firn, FIRN_23_PARENT).unwrap();
    let env = home().env;
    let host = Fake::default();
    assert_eq!(run(&["--lock".into(), firn.display().to_string()], &env, &host), 1);
    let clean = dir.path().join("clean.lock");
    std::fs::write(&clean, include_str!("../../../../flake.lock")).unwrap();
    assert_eq!(run(&["--lock".into(), clean.display().to_string()], &env, &host), 0);
}


fn text(findings: &[Finding]) -> String {
    findings.iter().map(|finding| format!("{} {}", if finding.ok { "ok" } else { "FAIL" }, finding.text)).collect::<Vec<_>>().join("\n")
}

#[test]
fn doctor_nix_probe_names_the_missing_ld_wrapper_the_toolchain_and_an_unrooted_shell() {
    let home = home();
    let host = Fake::default();
    let report = text(&probe(&home.env, &host));
    assert!(report.contains("FAIL rustup toolchain 1.96-x86_64-unknown-linux-gnu needs rustup-1.29.0/nix-support/ld-wrapper.sh"), "{report}");
    assert!(report.contains(&format!("ld.lld names {}", wrapper())), "{report}");
    assert!(report.contains("FAIL dev shell proj-") && report.contains("not rooted"), "{report}");
    assert!(report.contains("ok dev shell proj-") && report.contains("enters offline"), "{report}");
    assert!(host.calls.borrow().iter().any(|call| call.starts_with("nix develop ") && call.contains("--offline --no-update-lock-file") && call.ends_with("-c true")));
}

#[test]
fn doctor_nix_probe_reports_a_shell_that_does_not_enter_offline_and_a_rooted_one() {
    let home = home();
    let shell = crate::nix_gc::live_shells(&home.env.code).remove(0);
    std::fs::create_dir_all(home.env.dev_shell_roots()).unwrap();
    std::fs::create_dir(home.env.dev_shell_root(&shell)).unwrap();
    let host = Fake { fail: Some("develop"), ..Fake::default() };
    let report = text(&probe(&home.env, &host));
    assert!(report.contains("ok dev shell proj-") && report.contains(": rooted"), "{report}");
    assert!(report.contains("FAIL dev shell proj-") && report.contains("enters offline: develop failed"), "{report}");
}
