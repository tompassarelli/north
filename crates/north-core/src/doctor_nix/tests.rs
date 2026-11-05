use super::*;

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


