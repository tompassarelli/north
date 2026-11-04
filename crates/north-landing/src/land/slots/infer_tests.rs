use super::*;
use proptest::prelude::*;

fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

const FLAKE_WITH_CHECKS: &str = "{ outputs = { self }: { checks.x86_64-linux.default = 1; }; }";
const FLAKE_WITH_SHELL: &str = "{ outputs = { self }: { devShells.x86_64-linux.default = 1; }; }";

#[test]
fn a_tree_without_a_known_kind_has_no_checks() {
    assert!(infer(&files(&["README.md", "src/main.c", "Makefile"]), None).is_empty());
    assert!(infer(&[], None).is_empty());
}

#[test]
fn a_cargo_manifest_at_the_root_infers_test_then_clippy() {
    assert_eq!(
        infer(&files(&["Cargo.toml", "src"]), None),
        [
            "command -v cargo >/dev/null || exit 75; cargo test --locked --offline",
            "command -v cargo >/dev/null || exit 75; cargo clippy --all-targets --locked --offline -- -D warnings",
        ]
    );
}

#[test]
fn a_cargo_manifest_below_the_root_infers_nothing() {
    assert!(infer(&files(&["crates/a/Cargo.toml"]), None).is_empty());
}

#[test]
fn a_package_with_a_bun_lockfile_infers_bun_test() {
    assert_eq!(infer(&files(&["package.json", "bun.lock"]), None), ["command -v bun >/dev/null || exit 75; bun test"]);
    assert!(infer(&files(&["package.json"]), None).is_empty());
    assert!(infer(&files(&["bun.lock"]), None).is_empty());
    assert!(infer(&files(&["package.json", "package-lock.json"]), None).is_empty());
}

#[test]
fn a_flake_with_checks_infers_nix_flake_check() {
    assert_eq!(infer(&files(&["flake.nix"]), Some(FLAKE_WITH_CHECKS)), ["command -v nix >/dev/null || exit 75; nix flake check --offline --no-update-lock-file"]);
    assert!(infer(&files(&["flake.nix"]), Some(FLAKE_WITH_SHELL)).is_empty());
    assert!(infer(&files(&["flake.nix"]), Some("{ # checks are off\n outputs = 1; }")).is_empty());
    assert!(infer(&files(&["flake.nix"]), Some("{ outputs = { mychecks = 1; rechecks = 2; }; }")).is_empty());
    assert!(infer(&files(&["flake.nix"]), None).is_empty());
    assert!(infer(&files(&[]), Some(FLAKE_WITH_CHECKS)).is_empty());
}

#[test]
fn kinds_combine_in_a_fixed_order() {
    let found = infer(&files(&["flake.nix", "bun.lock", "package.json", "Cargo.toml"]), Some(FLAKE_WITH_CHECKS));
    assert_eq!(found.len(), 4);
    assert!(found[0].ends_with("cargo test --locked --offline"));
    assert!(found[1].contains("cargo clippy"));
    assert!(found[2].ends_with("bun test"));
    assert!(found[3].contains("nix flake check"));
}

#[test]
fn a_flake_dev_shell_wraps_the_toolchain_checks_and_not_the_flake_check() {
    let source = "{ devShells.default = 1; checks.default = 2; }";
    let found = infer(&files(&["flake.nix", "Cargo.toml", "package.json", "bun.lock"]), Some(source));
    assert_eq!(found.len(), 4);
    for check in &found[..3] {
        assert!(check.starts_with("command -v nix >/dev/null || exit 75; nix develop --offline --no-update-lock-file -c bash -euc '"), "{check}");
        assert!(check.contains("exit 75; "), "{check}");
    }
    assert!(found[0].ends_with("cargo test --locked --offline'"));
    assert!(found[3].starts_with("command -v nix >/dev/null || exit 75; nix flake check"));
}

#[test]
fn a_missing_toolchain_exits_with_the_infrastructure_status() {
    let check = &infer(&files(&["Cargo.toml"]), None)[0];
    let output = std::process::Command::new("bash").args(["-c", &format!("PATH=/nonexistent; {check}")]).output().unwrap();
    assert_eq!(output.status.code(), Some(75));
}

fn tree() -> impl Strategy<Value = (Vec<String>, Option<String>)> {
    let names = prop::sample::subsequence(
        vec!["Cargo.toml", "Cargo.lock", "package.json", "bun.lock", "flake.nix", "README.md", "crates/a/Cargo.toml", "web/package.json", "web/bun.lock"],
        0..=9,
    );
    let flake = prop::option::of(prop::sample::select(vec![FLAKE_WITH_CHECKS, FLAKE_WITH_SHELL, "{ checks = 1; devShell = 2; }", "{}"]));
    (names, flake).prop_map(|(names, flake)| (names.into_iter().map(str::to_owned).collect(), flake.map(str::to_owned)))
}

proptest! {
    #[test]
    fn the_same_tree_infers_the_same_checks_in_any_listing_order((names, flake) in tree(), seed in any::<u64>()) {
        let mut shuffled = names.clone();
        let length = shuffled.len().max(1);
        shuffled.rotate_left((seed as usize) % length);
        shuffled.reverse();
        shuffled.extend(names.iter().cloned());
        prop_assert_eq!(infer(&names, flake.as_deref()), infer(&shuffled, flake.as_deref()));
        prop_assert_eq!(infer(&names, flake.as_deref()), infer(&names, flake.as_deref()));
    }

    #[test]
    fn the_checks_are_distinct_and_follow_the_kind_order((names, flake) in tree()) {
        let found = infer(&names, flake.as_deref());
        let mut distinct = found.clone();
        distinct.sort();
        distinct.dedup();
        prop_assert_eq!(distinct.len(), found.len());
        let rank = |check: &String| {
            ["cargo test", "cargo clippy", "bun test", "nix flake check"].iter().position(|kind| check.contains(kind)).unwrap()
        };
        prop_assert!(found.windows(2).all(|pair| rank(&pair[0]) < rank(&pair[1])));
    }

    #[test]
    fn only_root_markers_decide_the_checks((names, flake) in tree()) {
        let root: Vec<String> = names.iter().filter(|name| !name.contains('/')).cloned().collect();
        prop_assert_eq!(infer(&names, flake.as_deref()), infer(&root, flake.as_deref()));
    }

    #[test]
    fn a_tree_has_checks_exactly_when_it_holds_a_known_kind((names, flake) in tree()) {
        let has = |name: &str| names.iter().any(|file| file == name);
        let flake_checks = has("flake.nix") && flake.as_deref().is_some_and(|source| names_attribute(source, "checks"));
        let expected = has("Cargo.toml") || (has("package.json") && has("bun.lock")) || flake_checks;
        prop_assert_eq!(!infer(&names, flake.as_deref()).is_empty(), expected);
    }
}
