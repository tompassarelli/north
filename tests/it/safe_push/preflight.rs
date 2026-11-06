use north_core::foundation::shim;

use super::{Real, Run, Scratch};

#[test]
fn a_code_health_refusal_does_not_block_a_dry_run_and_code_health_never_runs_before_the_landing() {
    let scratch = Scratch::new();
    let fixture = Real::new(&scratch, "code-health");
    let stub = scratch.path().join("code-health-stub");
    let trace = scratch.path().join("code-health-trace");
    shim::write_script(&stub, "printf 'code-health %s\\n' \"$*\" >>\"$CODE_HEALTH_TEST_TRACE\"\nexit \"${CODE_HEALTH_TEST_EXIT:-0}\"\n");
    let observed = Run::new(&scratch, &fixture.repo, &["--dry-run", "--to", "main"])
        .env("SAFE_PUSH_CODE_HEALTH", &stub)
        .env("CODE_HEALTH_TEST_EXIT", "1")
        .env("CODE_HEALTH_TEST_TRACE", &trace)
        .output();
    assert_eq!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains("safe-push: ✓ clean (--dry-run)"), "{}", observed.output);
    assert!(std::fs::read_to_string(&trace).unwrap_or_default().is_empty(), "code-health ran before the landing");
}
