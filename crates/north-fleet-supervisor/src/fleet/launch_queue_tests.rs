use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::model::{LaunchRequest, ProviderName};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtimes::Runtimes;

fn request(name: &str) -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: name.into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: PathBuf::from("/tmp"),
        model: None,
        effort: None,
        prompt: "work".into(),
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
        slots: Default::default(),
    }
}

#[test]
fn a_spend_refused_launch_reads_back_from_the_queue_urgent_first_and_leaves_when_it_starts() {
    let home = tempfile::tempdir().unwrap();
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap();
    fleet.queue_launch(&QueuedLaunch::new(request("dispatch-d1"), Priority::Normal, 20, "week limit".into())).unwrap();
    fleet.queue_launch(&QueuedLaunch::new(request("urgent-u1"), Priority::Urgent, 30, "week limit".into())).unwrap();

    let queued = fleet.queued_launches().unwrap();

    assert_eq!(queued.iter().map(|queued| queued.name.as_str()).collect::<Vec<_>>(), ["urgent-u1", "dispatch-d1"]);
    assert!(fleet.remove_queued("urgent-u1").unwrap());
    assert_eq!(fleet.queued_launches().unwrap().iter().map(|queued| queued.name.as_str()).collect::<Vec<_>>(), ["dispatch-d1"]);
}
