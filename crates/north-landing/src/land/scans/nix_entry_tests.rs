use super::*;

const PRE_D1_CHECK_OWNERS: &str = "#!/usr/bin/env bash\nset -euo pipefail\nroots=(--north-root \"$PWD\")\n\n\n\n\n\nexec nix develop -c cargo run --offline --bin north -- config agents check-owners \"${roots[@]}\"\n";
const PRE_D1_CITATION_CHECK: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\n\n\n\n\nexec nix develop -c cargo run --offline --bin north -- citation-check --root \"$PWD\"\n";

fn lines(path: &str, text: &str, dev_shell: bool) -> Vec<usize> {
    findings(path, text, dev_shell).into_iter().map(|finding| finding.line).collect()
}

#[test]
fn host_nix_entry_refuses_the_pre_d1_check_scripts() {
    assert_eq!(lines("harness/scripts/check-owners.sh", PRE_D1_CHECK_OWNERS, true), [9]);
    assert_eq!(lines("harness/scripts/citation-check.sh", PRE_D1_CITATION_CHECK, true), [8]);
    let finding = &findings("harness/scripts/check-owners.sh", PRE_D1_CHECK_OWNERS, false)[0];
    assert!(finding.reason.contains("`north dev-shell run --`"), "{}", finding.reason);
}

#[test]
fn host_nix_entry_refuses_every_literal_entry_in_scripts_and_land_check_lines() {
    for (path, text) in [
        ("bin/new-tool", "nix-shell --run 'cargo test'\n"),
        ("harness/lib/x.sh", "out=$(nix develop --offline -c cargo --version)\n"),
        ("harness/scripts/x.sh", "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo test\n"),
        ("harness/scripts/x.sh", "/opt/tools/bin/nix develop -c true\n"),
        (".safe-push", "[land]\n\tcheck = ** :: nix develop -c bash harness/scripts/x.sh\n"),
        (".safe-push", "[mutant]\n\trun = nix develop --no-update-lock-file -c bash harness/scripts/mutant-tests.sh\n"),
    ] {
        assert_eq!(findings(path, text, false).len(), 1, "{path}: {text}");
    }
}

#[test]
fn host_nix_entry_refuses_ad_hoc_toolchains_only_beside_a_dev_shell() {
    for text in ["nix shell nixpkgs#pkg-config -c cargo build\n", "exec nix run nixpkgs#hello\n", "nix-shell -p pkg-config --run make\n"] {
        let found = findings("bin/land-shell", text, true);
        assert_eq!(found.len(), 1, "{text}");
        assert!(found[0].reason.contains("ad-hoc nixpkgs toolchain"), "{text}");
    }
    assert!(findings("bin/tool", "nix shell nixpkgs#pkg-config -c cargo build\n", false).is_empty());
    assert!(has_dev_shell("{ outputs = _: { devShells.x86_64-linux.default = {}; }; }"));
    assert!(!has_dev_shell("{ outputs = _: { packages.x86_64-linux.default = {}; }; }"));
}

#[test]
fn host_nix_entry_allows_the_entries_and_mentions() {
    for (path, text) in [
        ("bin/land-shell", "exec \"$depbox\" run -- nix develop --offline --no-update-lock-file -c \"$@\"\n"),
        ("bin/head-north", "  \"$root/bin/land-shell\" --dev -- cargo build -q --locked --offline --bin north >&2\n"),
        (".safe-push", "\tcheck = src/ :: CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets\n"),
        ("harness/scripts/canary.sh", "    command=$(sed -E 's#(\\./bin/)?depbox run -- ##g; s#nix develop -c ##g' <<<\"$command\")\n"),
        ("harness/scripts/x.sh", "# never nix develop here\n"),
        ("harness/scripts/x.sh", "exec ./bin/head-north dev-shell run -- cargo test\n"),
        ("docs/x.md", "nix develop -c cargo test\n"),
    ] {
        assert!(!in_scope(path) || findings(path, text, true).is_empty(), "{path}: {text}");
    }
}
