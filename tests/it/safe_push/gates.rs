use super::{CLEAN, Fixture};

#[test]
fn a_failing_land_check_and_a_secret_are_refused_once_naming_both() {
    let fixture = Fixture::new();
    fixture.land_checks(&["file.txt :: printf \"FAIL boundary\\n\" >&2; exit 9"], "a test that fails");
    fixture.commit("file.txt", "changed\n", "change under test beside a secret fixture");
    let before = fixture.remote_state();
    fixture.leak_secret();
    let ran = fixture.run(&["--dry-run", "--to", "main"]);
    ran.expect(false, &["safe-push: ✗ 2 blocking check(s) failed: land checks; secret scan; NOT pushed.", "safe-push: ✗ SECRET DETECTED"]);
    assert_eq!(ran.output.lines().filter(|line| line.contains("✗ ") && line.contains(" blocking check(s) failed")).count(), 1, "the refusal was not reported once: {}", ran.output);
    assert_eq!(fixture.remote_state(), before, "a refused landing published");
}

#[test]
fn a_north_skill_change_lands_with_no_reviewed_trailer() {
    let fixture = Fixture::new();
    fixture.origin_named_north();
    fixture.commit("harness/skills/example/SKILL.md", "changed\n", "change a skill");
    fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[CLEAN]);
}
