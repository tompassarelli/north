use super::*;
use super::sync::settle;
use north_base::agents_sync::POLICY_HELD;
use std::os::unix::fs::symlink;

fn steps(sync_exit: u8) -> Vec<(&'static str, Command)> {
    let mut sync = Command::new("sh");
    sync.args(["-c", &format!("echo 'agents sync: startup tokens grew 3974→4525 with no measured conformance gain'; echo 'agents sync: generation not published'; exit {sync_exit}")]);
    vec![("health check", Command::new("true")), ("agents sync", sync)]
}

#[test]
fn an_unpublished_generation_keeps_the_code_runtime_and_notifies_the_proxy_once_per_cause() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let (old, new) = ("a".repeat(40), "b".repeat(40));
    for sha in [&old, &new] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(&old), layout.current()).unwrap();
    let previous = layout::link(&old);
    let mut notices = Vec::new();
    for _ in 0..2 {
        assert!(settle(&layout, Some(&previous), &new, steps(POLICY_HELD), &mut |text: &str| notices.push(text.to_owned())).is_ok());
        assert_eq!(fs::read_link(layout.current()).unwrap(), layout::link(&new));
    }
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("startup tokens grew 3974→4525") && notices[0].contains("last published one stays live"), "{}", notices[0]);

    assert!(settle(&layout, Some(&previous), &new, steps(0), &mut |text: &str| notices.push(text.to_owned())).is_ok());
    assert!(settle(&layout, Some(&previous), &new, steps(POLICY_HELD), &mut |text: &str| notices.push(text.to_owned())).is_ok());
    assert_eq!(notices.len(), 2, "a hold after a published generation is news again");

    assert!(settle(&layout, Some(&previous), &new, steps(1), &mut |text: &str| notices.push(text.to_owned())).is_ok());
    assert_eq!(fs::read_link(layout.current()).unwrap(), layout::link(&new), "an agents sync failure never rolls the code back");
    assert!(notices.len() == 3 && notices[2].contains("agents sync exited 1"), "{notices:?}");

    let mut unhealthy = steps(0);
    unhealthy[0].1 = Command::new("false");
    assert!(settle(&layout, Some(&previous), &new, unhealthy, &mut |text: &str| notices.push(text.to_owned())).is_err());
    assert_eq!(fs::read_link(layout.current()).unwrap(), previous, "a failed health check still rolls the code back");
}

#[test]
fn a_held_generation_names_the_gates_refusal_not_only_that_the_gate_refused() {
    let logged = "session start 4100 tokens\nskills/shared/github/SKILL.md description is 1030 bytes, 6 over its 1024-byte cap\nagents sync: generation not published; the last published generation stays live\n";
    assert_eq!(super::sync::held_cause(logged), "skills/shared/github/SKILL.md description is 1030 bytes, 6 over its 1024-byte cap");
}
