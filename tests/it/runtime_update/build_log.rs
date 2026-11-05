use super::harness::*;
use std::fs;
use std::path::Path;

const CARGO_RAN: &str = "mkdir -p \"$CARGO_TARGET_DIR/cargo-timings\"\ntouch \"$CARGO_TARGET_DIR/cargo-timings/cargo-timing.html\"\n";

fn build_log(host: &Host) -> std::path::PathBuf {
    host.path("state/north/runtime-build-logs").join(format!("{}.log", host.target))
}

#[test]
fn a_failed_build_keeps_its_compiler_output_in_the_log_its_failure_line_names() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(&host.path("bin/agents"), "case \"$1\" in sync) exit 0;; esac\n");
    script(&host.path("bin/capacity-north"), &format!("{CARGO_RAN}echo 'error[E0599]: no method named fixture_marker' >&2\nexit 101\n"));
    let output = host.update(0, 0);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{stderr}");
    let log = build_log(&host);
    assert!(stderr.contains(&format!("build log: {}", log.display())), "{stderr}");
    assert!(fs::read_to_string(&log).unwrap().contains("fixture_marker"));
    let causes = fs::read_to_string(host.path("state/north/runtime-failed-causes")).unwrap();
    assert!(causes.contains(&log.display().to_string()), "{causes}");
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

#[test]
fn a_build_that_links_against_stale_release_incremental_output_clears_it_and_builds_once_more() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(&host.path("bin/agents"), "case \"$1\" in sync) exit 0;; esac\n");
    script(
        &host.path("bin/capacity-north"),
        &format!(
            "n=$(cat \"$TEST_COUNT\" 2>/dev/null || echo 0)\necho $((n + 1)) >\"$TEST_COUNT\"\n{CARGO_RAN}\
             release=\"$CARGO_TARGET_DIR/release\"\n\
             if [ \"$n\" = 0 ]; then mkdir -p \"$release/incremental/north_core-x/s-1\" \"$release/.fingerprint/serde-1\"; echo 'rust-lld: error: undefined hidden symbol: anon.1.2.llvm.3' >&2; exit 101; fi\n\
             [ -e \"$release/incremental\" ] && exit 102\n[ -d \"$release/.fingerprint/serde-1\" ] || exit 103\n\
             for program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"$release/$program\"; chmod +x \"$release/$program\"; done\n"
        ),
    );
    let count = host.path("count");
    let output = host.north_with(&["runtime", "update"], &[("TEST_COUNT", count.to_str().unwrap())]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("stale release incremental output") && stdout.contains("building once more"), "{stdout}");
    assert_eq!(fs::read_to_string(&count).unwrap().trim(), "2");
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(!build_log(&host).exists());
    assert!(!host.path("state/north/runtime-failed").exists());
}
