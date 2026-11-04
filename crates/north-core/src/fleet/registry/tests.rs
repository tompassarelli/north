use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::error::NorthError;
use crate::fleet::model::{Effort, Machine};
use crate::fleet::store::Claim;
use crate::fleet::prompt::PromptMode;
use crate::providers::claude::fleet::Claude;

fn fleet(home: &Path, profiles: &str) -> Fleet {
    fleet_claiming(home, profiles, Arc::new(claims::Held::default()))
}

fn fleet_claiming(home: &Path, profiles: &str, held: Arc<claims::Held>) -> Fleet {
    let local = home.join(".local/state/north/profiles/claude-a");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join(".credentials.json"), "{}").unwrap();
    fs::write(local.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"local-login"}}"#).unwrap();
    let policy = crate::pace::target::home_policy_path(home);
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    let here = crate::platform::host::name();
    fs::write(&policy, format!("[accounts.claude.profiles.a]\n\"{here}\" = {{ path = \"~/.local/state/north/profiles/claude-a\" }}\n{profiles}")).unwrap();
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
    Fleet::at_claiming(home.into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, held).unwrap()
}

fn on_far() -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: "worker".into(),
        provider: Claude.provider(),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt: String::new(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        placement: None,
        machine: Some(Machine { name: "far".into(), ssh: "tom@far".into() }),
        capacity_need: None,
    }
}

fn remote_command(fleet: &Fleet) -> String {
    let request = on_far();
    let account = fleet.account(&request).unwrap();
    let record = SessionRecord { machine: request.machine.clone(), ..serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null})).unwrap() };
    let spec = remote::placed(Claude.host(&account, &request, Opening::Fresh(None), "").unwrap(), &record).unwrap();
    spec.argv.last().unwrap().clone()
}

#[test]
fn a_session_placed_on_another_machine_runs_on_that_machines_declared_token_whatever_this_hosts_login() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "far = { path = \"~/far-a\", token_file = \"~/tokens/claude-a\" }\n");

    let command = remote_command(&fleet);

    let far_home = home.path().join("far-a").display().to_string();
    let far_token = home.path().join("tokens/claude-a").display().to_string();
    assert!(command.contains(&format!("'--setenv=CLAUDE_CONFIG_DIR={far_home}'")), "{command}");
    assert!(command.contains("CLAUDE_CODE_OAUTH_TOKEN=$(cat -- \"$0\")") && command.contains(&format!("'{far_token}'")), "{command}");
    assert!(!command.contains("profiles/claude-a"), "{command}");
}

#[test]
fn a_session_placed_on_another_machine_without_a_declared_token_runs_on_that_machines_declared_home() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "far = { path = \"/srv/far-a\" }\n");

    let command = remote_command(&fleet);

    assert!(command.contains("'--setenv=CLAUDE_CONFIG_DIR=/srv/far-a'"), "{command}");
    assert!(!command.contains("CLAUDE_CODE_OAUTH_TOKEN") && !command.contains("profiles/claude-a"), "{command}");
}

#[test]
fn a_label_with_no_profile_for_the_named_machine_is_refused_naming_its_owners() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "near = { path = \"~/near-a\" }\n");

    match fleet.account(&on_far()) {
        Err(NorthError::Refused(why)) => assert!(why.contains("not far") && why.contains("near"), "{why}"),
        other => panic!("a label undeclared for far was placed there: {other:?}"),
    }
}

fn chain(fleet: &Fleet, links: &[(&str, Option<&str>)]) {
    for (name, parent) in links {
        let session = fleet.root.join(name);
        fs::create_dir_all(&session).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "parent": parent});
        fs::write(session.join("session.json"), record.to_string()).unwrap();
    }
}

fn launch_refusal(fleet: &Fleet, caller: &str) -> Option<String> {
    match fleet.admit_launch_from(Some(caller)) {
        Ok(()) => None,
        Err(NorthError::Refused(why)) => Some(why),
        Err(other) => panic!("{caller} was refused for the wrong reason: {other:?}"),
    }
}

#[test]
fn a_launch_from_a_session_two_levels_below_the_lead_is_refused_naming_its_depth() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("lead", None), ("worker", Some("lead")), ("judge", Some("worker"))]);

    let why = launch_refusal(&fleet, "judge").expect("a session two levels below the lead launched");

    assert!(why.contains("judge is 2 levels below the lead"), "{why}");
}

#[test]
fn a_launch_from_the_lead_or_one_level_below_it_is_admitted() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("lead", None), ("worker", Some("lead")), ("judge", Some("worker"))]);

    assert_eq!(launch_refusal(&fleet, "lead"), None);
    assert_eq!(launch_refusal(&fleet, "worker"), None);
    assert_eq!(launch_refusal(&fleet, "unrecorded"), None);
}

#[test]
fn a_session_whose_parent_record_is_gone_still_counts_that_parent_as_a_level() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("orphan", Some("forgotten-lead")), ("grandchild", Some("orphan"))]);

    assert_eq!(launch_refusal(&fleet, "orphan"), None);
    assert!(launch_refusal(&fleet, "grandchild").is_some());
}

#[test]
fn a_parent_cycle_is_refused_instead_of_walked_forever() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("a", Some("b")), ("b", Some("a"))]);

    assert!(launch_refusal(&fleet, "a").is_some());
}

#[test]
fn a_revived_session_shows_the_turns_and_final_report_after_a_frame_larger_than_a_read_window() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let session = fleet.root.join("revived");
    let record = serde_json::json!({"name": "revived", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "c", "host_dir": session, "stream_offset": 0, "machine": {"name": "far", "ssh": "tom@far"}});
    fs::create_dir_all(session.join("g1")).unwrap();
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let assistant = |text: &str| serde_json::json!({"type": "assistant", "session_id": "c", "message": {"content": [{"type": "text", "text": text}]}}).to_string();
    let strip = serde_json::json!({"type": "user", "session_id": "c", "message": {"content": [{"type": "tool_result", "content": "A".repeat(3 * crate::fleet::transcript::WINDOW_SIZE as usize)}]}}).to_string();
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": "c"}).to_string();
    let lines = [assistant("before the revive"), result.clone(), strip, assistant("final report"), result];
    fs::write(session.join("g1/stream.jsonl"), lines.map(|line| line + "\n").concat()).unwrap();

    let follower = fleet.follow("revived").unwrap();

    let replies: Vec<_> = follower.transcript().entries.iter().filter(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.as_str()).collect();
    assert_eq!(replies, ["before the revive", "final report"]);
}

#[test]
fn the_live_owners_are_the_unexited_sessions_names_conversations_and_provider_conversations() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    for (name, stopped, conversation) in [("parked", false, "c1"), ("gone", true, "c2")] {
        let session = fleet.root.join(name);
        fs::create_dir_all(&session).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": conversation, "stopped": stopped});
        fs::write(session.join("session.json"), record.to_string()).unwrap();
    }

    let owners = fleet.live_owners().unwrap();

    assert_eq!(owners.iter().map(String::as_str).collect::<Vec<_>>(), ["c1", "claude:c1", "parked"]);
}

#[test]
fn an_unreadable_registry_is_not_an_empty_fleet() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fs::remove_dir_all(&fleet.root).unwrap();

    assert_eq!(fleet.live_owners(), None);
}

fn parked(fleet: &Fleet, name: &str, stopped: bool, report: &str) {
    parked_turn(fleet, name, stopped, report, true);
}

fn parked_turn(fleet: &Fleet, name: &str, stopped: bool, report: &str, ended: bool) {
    let session = fleet.root.join(name);
    fs::create_dir_all(session.join("g1")).unwrap();
    let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "host_dir": session, "stream_offset": 0, "stopped": stopped});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let assistant = serde_json::json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": report}]}});
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": name});
    let closing = if ended { format!("{result}\n") } else { String::new() };
    fs::write(session.join("g1/stream.jsonl"), format!("{assistant}\n{closing}")).unwrap();
    fleet.status(&fleet.session(name).unwrap());
}

#[test]
fn a_reboot_strands_the_unstopped_sessions_that_had_not_reported_done_or_idled_out_and_only_those() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    parked(&fleet, "waiting", false, "Not done: the build is queued; I resume when it ends.");
    parked(&fleet, "mid-turn", false, "Reading the relay code now.");
    parked(&fleet, "finished", false, "Done: landed 4f2a9c1 on origin/main.");
    parked(&fleet, "stopped", true, "Not done: stopped by the lead.");
    parked(&fleet, "idle-parked", false, "Not done: waiting on the lead.");
    let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
    fs::File::options().write(true).open(fleet.root.join("idle-parked/g1/stream.jsonl")).unwrap().set_modified(two_hours_ago).unwrap();
    parked_turn(&fleet, "silent-tool", false, "Running the full test suite.", false);
    fs::File::options().write(true).open(fleet.root.join("silent-tool/g1/stream.jsonl")).unwrap().set_modified(two_hours_ago).unwrap();
    let after_writes = crate::fleet::runtime::now_ms() + 60_000;
    let before_writes = 1_000;

    let mut stranded = fleet.rebooted(after_writes).unwrap();
    stranded.sort();
    let not_stranded = fleet.rebooted(before_writes).unwrap();

    assert_eq!(stranded, ["mid-turn", "silent-tool", "waiting"]);
    assert!(not_stranded.is_empty(), "a session written after the boot ran after it: {not_stranded:?}");
}

#[test]
fn a_sonnet_session_starts_and_revives_at_high_effort_and_refuses_any_other() {
    let sonnet = Some("claude-sonnet-5-5");
    assert_eq!(efforts::taken(&Claude, sonnet, None).unwrap(), Some(Effort::High), "an unset effort is the account default medium, so Sonnet is pinned to high");
    assert_eq!(efforts::taken(&Claude, sonnet, Some(Effort::High)).unwrap(), Some(Effort::High));
    for other in [Effort::Low, Effort::Medium, Effort::Xhigh, Effort::Max] {
        assert!(efforts::taken(&Claude, sonnet, Some(other)).unwrap_err().to_string().contains("runs only at high effort"), "{other:?}");
    }
    assert_eq!(efforts::taken(&Claude, Some("claude-opus-5-5"), Some(Effort::Medium)).unwrap(), Some(Effort::Medium));
    assert_eq!(efforts::taken(&Claude, None, None).unwrap(), None);
    let stored = |effort: Option<Effort>| SessionRecord { model: sonnet.map(str::to_owned), effort, ..serde_json::from_value(serde_json::json!({"name": "old", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null})).unwrap() };
    assert_eq!(efforts::revived(&stored(Some(Effort::Medium)), &Claude, false, None).unwrap(), Some(Effort::High), "a stored Sonnet at medium is restored to high");
    assert_eq!(efforts::revived(&stored(None), &Claude, false, None).unwrap(), Some(Effort::High));
    assert!(efforts::revived(&stored(None), &Claude, false, Some(Effort::Medium)).is_err());
}

#[test]
fn a_session_that_exited_holding_a_claim_leaves_the_issue_unowned_while_parked_and_running_sessions_keep_theirs() {
    let home = tempfile::tempdir().unwrap();
    let held = Arc::new(claims::Held::default());
    let fleet = fleet_claiming(home.path(), "", held.clone());
    let running = std::process::id();
    for (name, stopped, pid) in [("exited", true, None), ("parked", false, None), ("running", false, Some(running))] {
        let session = fleet.root.join(name);
        fs::create_dir_all(&session).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": pid, "stopped": stopped});
        fs::write(session.join("session.json"), record.to_string()).unwrap();
        held.hold(&format!("north#{name}"), name).unwrap();
    }
    held.hold("north#elsewhere", "someone-else").unwrap();

    let released = fleet.release_exited_claims().unwrap();

    let mut owners = held.holders().unwrap();
    owners.sort();
    assert_eq!(released, 1);
    assert_eq!(
        owners,
        [("north#elsewhere", "someone-else"), ("north#parked", "parked"), ("north#running", "running")].map(|(item, owner)| (item.to_owned(), owner.to_owned()))
    );
}

#[derive(Default)]
struct Unreachable(std::sync::Mutex<Vec<(crate::fleet::store::SessionRow, Claim)>>);

impl Registrar for Unreachable {
    fn register(&self, row: &crate::fleet::store::SessionRow, claim: Claim) -> NorthResult<()> {
        self.0.lock().unwrap().push((row.clone(), claim));
        Err(NorthError::Refused(format!("a launch registers {} in the fleet store on the ledger host first, and it did not answer: ssh nexus: timed out", row.name)))
    }
}

#[test]
fn a_launch_registers_its_fleet_row_before_it_starts_and_refuses_when_the_fleet_store_does_not_answer() {
    let home = tempfile::tempdir().unwrap();
    let store = Arc::new(Unreachable::default());
    let fleet = fleet(home.path(), "").registering(store.clone());

    let refused = fleet.launch(&LaunchRequest { machine: None, ..on_far() });

    assert!(matches!(refused, Err(NorthError::Refused(why)) if why.contains("did not answer")));
    assert!(!fleet.session_dir("worker").unwrap().exists(), "a refused launch starts nothing");
    let calls = store.0.lock().unwrap();
    assert_eq!(calls.iter().map(|(row, claim)| (row.name.as_str(), row.host.as_str(), row.status.as_str(), *claim)).collect::<Vec<_>>(), [("worker", fleet.host(), "starting", Claim::Fresh)]);
}

#[test]
fn a_take_over_claims_the_row_under_the_name_and_a_refused_one_leaves_the_holder_in_place() {
    let home = tempfile::tempdir().unwrap();
    let store = Arc::new(Unreachable::default());
    let fleet = fleet(home.path(), "").registering(store.clone());
    let held = fleet.session_dir("worker").unwrap();
    fs::create_dir_all(&held).unwrap();
    fs::write(held.join("session.json"), serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "c0ffee"}).to_string()).unwrap();

    assert!(fleet.take_over(&LaunchRequest { machine: None, ..on_far() }).is_err());

    assert_eq!(store.0.lock().unwrap().iter().map(|(row, claim)| (row.name.as_str(), *claim)).collect::<Vec<_>>(), [("worker", Claim::TakeOver)]);
    assert_eq!(fleet.record("worker").unwrap().conversation.as_deref(), Some("c0ffee"), "the holder is not archived when the store refuses");
}

fn hosted(fleet: &Fleet, name: &str, model: Option<&str>, effort: &str) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new("sleep").arg("30").process_group(0).stdin(std::process::Stdio::null()).spawn().unwrap();
    let session = fleet.root.join(name);
    fs::create_dir_all(session.join("g1")).unwrap();
    fs::write(session.join("host.pid"), child.id().to_string()).unwrap();
    let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": model, "effort": effort, "created_ms": 1, "host_pid": child.id(), "conversation": "c1", "host_dir": session, "stream_offset": 0});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": "c1"});
    fs::write(session.join("g1/stream.jsonl"), format!("{result}\n")).unwrap();
    child
}

#[test]
fn a_relaunch_stops_the_host_then_restarts_the_same_conversation_on_the_new_model_and_effort() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let mut host = hosted(&fleet, "worker", Some("opus"), "low");
    let pid = host.id();

    let restarted = fleet
        .relaunch_with("worker", Some("sonnet"), None, |name| {
            let held = fleet.record(name).unwrap();
            assert!(held.stopped && !alive(pid), "the restart began while the old host still ran");
            Ok(held)
        })
        .unwrap();
    host.wait().unwrap();

    assert_eq!((restarted.model.as_deref(), restarted.effort, restarted.conversation.as_deref()), (Some("sonnet"), Some(Effort::High), Some("c1")));
    let kept = fleet.record("worker").unwrap();
    assert_eq!((kept.model, kept.effort, kept.conversation.as_deref()), (Some("sonnet".into()), Some(Effort::High), Some("c1")));
}

#[test]
fn a_relaunch_effort_alone_keeps_the_model_and_a_refused_change_leaves_the_session_running() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let mut host = hosted(&fleet, "worker", Some("opus"), "low");

    let refusals = [
        fleet.relaunch_with("worker", Some("gpt-9"), None, |_| panic!("restarted on a model the runtime lacks")),
        fleet.relaunch_with("worker", Some("sonnet"), Some(Effort::Low), |_| panic!("restarted Sonnet below high")),
        fleet.relaunch_with("worker", None, None, |_| panic!("restarted with nothing to change")),
    ];
    for refusal in &refusals {
        assert!(matches!(refusal, Err(NorthError::Usage(_))), "{refusal:?}");
    }
    assert!(!fleet.record("worker").unwrap().stopped && alive(host.id()), "a refused change stopped the session");

    let restarted = fleet.relaunch_with("worker", None, Some(Effort::Max), |name| fleet.record(name)).unwrap();
    host.wait().unwrap();
    assert_eq!((restarted.model.as_deref(), restarted.effort), (Some("opus"), Some(Effort::Max)));
}

#[test]
fn an_interactive_hand_over_stops_the_fleet_host_before_it_runs_the_provider_ui_on_the_same_conversation() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let mut host = hosted(&fleet, "worker", Some("opus"), "high");
    let pid = host.id();
    let mut launched = None;

    fleet
        .interactive("worker", |spec| {
            let held = fleet.record("worker").unwrap();
            launched = Some((held.stopped, alive(pid), spec.argv.clone(), spec.env.clone(), spec.cwd.clone()));
            Ok(())
        })
        .unwrap();
    host.wait().unwrap();

    let (stopped, host_alive, argv, env, cwd) = launched.expect("the interactive command never ran");
    assert!(stopped && !host_alive, "the UI would have started beside the fleet host");
    let words = argv.join(" ");
    assert!(words.contains("--resume c1") && words.contains("--model opus") && words.contains("--effort high"), "{words}");
    assert!(!argv.iter().any(|word| word == "-p" || word == "stream-json"), "{words}");
    assert!(env.iter().any(|(key, value)| key == "NORTH_FLEET_SESSION" && value == "worker"));
    assert_eq!(cwd, std::path::PathBuf::from("/tmp"));
}

#[test]
fn a_revive_and_a_second_hand_over_are_refused_while_the_interactive_ui_holds_the_conversation() {
    let home = tempfile::tempdir().unwrap();
    let proc = home.path().join("proc");
    for (pid, words) in [("11", "claude\0-p\0--resume\0worker\0--output-format\0stream-json"), ("12", "claude\0--resume\0other\0"), ("13", "north\0fleet\0list\0")] {
        fs::create_dir_all(proc.join(pid)).unwrap();
        fs::write(proc.join(pid).join("cmdline"), words).unwrap();
    }
    let fleet = fleet(home.path(), "").reading_proc(proc.clone());
    parked(&fleet, "worker", true, "Not done: stopped.");
    let held = fleet.session("worker").unwrap();
    assert!(fleet.refuse_beside_interactive(&held).is_ok(), "a stream host or another conversation is not the interactive UI");

    fs::create_dir_all(proc.join("14")).unwrap();
    fs::write(proc.join("14/cmdline"), "claude\0--resume\0worker\0--permission-mode\0bypassPermissions\0").unwrap();

    for refused in [fleet.revive("worker", None).map(drop), fleet.interactive("worker", |_| Ok(()))] {
        match refused {
            Err(NorthError::Refused(why)) => assert!(why.contains("interactive UI") && why.contains("process 14"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}
