use super::harness::*;
use std::path::Path;

#[test]
fn a_deploy_to_a_host_already_running_a_descendant_leaves_its_runtime_and_names_both_commits() {
    let host = Host::new();
    let checkout = host.path("checkout");
    git(&host.path("home"), &checkout, &["commit", "-q", "--allow-empty", "-m", "newer"]);
    let newer = git(&host.path("home"), &checkout, &["rev-parse", "HEAD"]);
    host.built(&host.target);
    host.built(&newer);
    host.select(&newer);
    script(&host.path("bin/rsync"), "true\n");
    script(
        &host.path("bin/ssh"),
        "while [[ $1 == -o ]]; do shift 2; done\n[[ $1 == -- ]] && shift\nshift\n[[ \"$2 $3\" == 'runtime update' ]] && AGENTS_ORCHESTRATION=\"$TEST_FAR_FLEET\" RESIDENT_CAP_HOST=plain exec \"$TEST_NORTH\" \"${@:2}\"\nexit 0\n",
    );
    let far_fleet = fleet(&host, "[hosts.plain]\nresident_cap = 1\n");
    let output = host.run(&["runtime", "deploy", "far"], 0, 0, &[("TEST_NORTH", env!("CARGO_BIN_EXE_north")), ("TEST_FAR_FLEET", far_fleet.to_str().unwrap())]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains(&format!("not moving plain back from {} to {}", &newer[..8], &host.target[..8])), "{stdout}");
    assert_eq!(host.current(), Path::new("runtimes").join(&newer));
}
