use crate::fleet::model::{LaunchRequest, ProviderName, Role};

use super::inherit_from;

fn launched(flags: &[&str]) -> LaunchRequest {
    let words: Vec<&str> = ["--provider", "claude", "--account", "a", "--name", "child"].iter().chain(flags).chain(&["probe the trusted mirror"]).copied().collect();
    super::super::launch::launch_parts(&[ProviderName::new("claude")], &words, Some("p-lead".into()), false).unwrap().request
}

/// On 2026-11-14 a lead launched a session with `--name` alone, which recorded role lead with no task and ran outside the confined domain.
#[test]
fn a_session_a_lead_or_worker_launches_is_a_worker_holding_its_launchers_task_unless_it_names_the_lead_role() {
    for launcher in [Role::Lead, Role::Worker] {
        let mut child = launched(&[]);
        inherit_from(&mut child, Some(launcher), Some("south#138".into()));
        assert_eq!((child.role(), child.task()), (Role::Worker, Some("south#138".into())), "launched by a {launcher:?}");
    }
    let mut named = launched(&["--role", "lead", "--task", "north#7"]);
    inherit_from(&mut named, Some(Role::Lead), Some("south#138".into()));
    assert_eq!((named.role(), named.task()), (Role::Lead, Some("north#7".into())));
    let mut from_proxy = launched(&[]);
    inherit_from(&mut from_proxy, Some(Role::Proxy), Some("south#138".into()));
    assert_eq!((from_proxy.role(), from_proxy.task()), (Role::Lead, None));
}
