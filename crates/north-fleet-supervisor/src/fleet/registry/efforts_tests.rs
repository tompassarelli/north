use std::path::Path;

use crate::fleet::model::{Effort, LaunchRequest, ProviderName};
use crate::fleet::prompt::PromptMode;

use super::super::fixtures::{fleet, standing_host};

fn request(cwd: &Path, name: &str, model: &str) -> LaunchRequest {
    LaunchRequest {
        verification: false,
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: name.into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: cwd.into(),
        model: Some(model.into()),
        effort: None,
        prompt: "start".into(),
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
        role: None,
        resume: None,
        compact_window: None,
        dispatched: false,
        surface: Default::default(),
        route: None,
    }
}

#[test]
fn a_launch_without_an_effort_flag_records_the_effort_it_runs_at() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path()).starting_hosts(standing_host);
    let opus = fleet.launch(&request(home.path(), "opus-lead", "claude-opus-5-5")).unwrap();
    assert_eq!(opus.effort, Some(Effort::Medium), "an Opus launch with no --effort records no effort");
    let sonnet = fleet.launch(&request(home.path(), "sonnet-lead", "claude-sonnet-5-5")).unwrap();
    assert_eq!(sonnet.effort, Some(Effort::High), "a Sonnet launch with no --effort records no effort");
    let fable = fleet.launch(&request(home.path(), "fable-lead", "claude-fable-5-2")).unwrap();
    assert_eq!(fable.effort, None, "a Fable launch has no catalog pin, so it launches at the client's default and records none");
}
