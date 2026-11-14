use std::sync::Arc;

use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::model::{LaunchRequest, ProviderName, Role, Surface};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtimes::Runtimes;

use super::super::Fleet;
use super::OpenedWindows;

/// On 2026-11-14 a take-over of the coordinator started a headless host and opened a window that only printed its transcript (north#1427).
#[test]
fn a_take_over_of_a_window_held_proxy_opens_its_interactive_client_on_the_assigned_conversation_with_the_first_prompt_and_binds_the_name_to_it() {
    let home = tempfile::tempdir().unwrap();
    let desk = Arc::new(OpenedWindows::default());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(())).showing_proxy_on(desk.clone());
    let request = LaunchRequest {
        verification: false,
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: "proxy".into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: home.path().into(),
        model: None,
        effort: None,
        prompt: "[north: take the fleet]".into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: Some(Role::Proxy),
        resume: None,
        compact_window: None,
        dispatched: false,
        surface: Surface::Window,
        route: None,
    };

    let launched = fleet.take_over(&request).unwrap_or_else(|error| panic!("{}", error.user_message()));

    let held = fleet.session("proxy").unwrap();
    let conversation = held.conversation.clone().expect("the launch assigned no conversation");
    assert_eq!(launched.conversation.as_deref(), Some(conversation.as_str()));
    assert_eq!(desk.opened(), [("proxy".to_owned(), ["client", "--session-id", &conversation, "[north: take the fleet]"].map(str::to_owned).to_vec())]);
    assert_eq!(held.host_pid, None, "a host started for a proxy held in a window");
    assert!(held.adopted(), "the name is not bound to the window's conversation: {held:?}");
}
