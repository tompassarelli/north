#![cfg(test)]

use super::worktree_remove::decide;
use crate::hook::{HookInput, Verdict};
use serde_json::json;
use std::fs;
use std::path::Path;

fn ticket(path: &Path, root: &Path, head: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ticket = json!({"origin": "https://github.com/tompassarelli/north", "root": root, "host": "gex45", "branch": "n706-claim-filed", "head": head, "session": "n-takeable", "queued_at": "2026-11-07T03:07:09Z", "red_fix": null});
    fs::write(path, ticket.to_string()).unwrap();
}

#[test]
fn removing_a_landed_lane_is_refused_while_its_second_ticket_waits_in_the_outbox() {
    let home = tempfile::tempdir().unwrap();
    let main = home.path().join("code/north/main");
    let lane = home.path().join("code/north/worktrees/n706-claim-filed");
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(&lane).unwrap();
    let queue = home.path().join("land-queue/de2e0e8479546ba9");
    ticket(&queue.join("outbox/taken/1794020468167698-n706-claim-filed.json"), &lane, "e9ba7b34354e96856170ba813dda0a5185d4edd7");
    let waiting = queue.join("outbox/1794020829623648-n706-claim-filed.json");
    ticket(&waiting, &lane, "ef0ef324027acc6752dd7476b7b3dd49c13e768f");
    let command = format!("rm -rf {0}/target.seeding-2305204 && git worktree remove {0} && git branch -D n706-claim-filed 2>&1 | tail -1", lane.display());
    let input = HookInput::parse(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": command}, "cwd": main}).to_string().as_bytes()).unwrap();
    let relative = HookInput::parse(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "git -C main worktree remove ../worktrees/n706-claim-filed"}, "cwd": main.parent().unwrap()}).to_string().as_bytes()).unwrap();
    let queues = home.path().join("land-queue");
    for call in [&input, &relative] {
        assert!(matches!(decide(call, &queues, "gex45"), Verdict::Deny(reason) if reason.contains("1794020829623648-n706-claim-filed.json")), "{:?}", call.command());
    }
    fs::create_dir_all(queue.join("outbox/taken")).unwrap();
    fs::rename(&waiting, queue.join("outbox/taken/1794020829623648-n706-claim-filed.json")).unwrap();
    assert!(matches!(decide(&input, &queues, "gex45"), Verdict::Deny(reason) if reason.contains("north lane remove")));
}
