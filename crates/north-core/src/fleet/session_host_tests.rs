use super::*;
use crate::claude_stream::{
    StreamFrame, decode_frame, encode_initialize, encode_interrupt, encode_user_message,
};
use std::sync::OnceLock;
use std::time::Duration;

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn fake_claude() -> String {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let profile = exe.parent().and_then(Path::parent).unwrap();
            let name = profile.file_name().and_then(|name| name.to_str()).unwrap();
            let status = std::process::Command::new(env!("CARGO"))
                .args(["build", "--quiet", "--locked", "--example", "fake_claude", "--profile"])
                .arg(if name == "debug" { "dev" } else { name })
                .arg("--manifest-path")
                .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
                .arg("--target-dir")
                .arg(profile.parent().unwrap())
                .status()
                .unwrap();
            assert!(status.success());
            profile.join("examples/fake_claude").to_string_lossy().into_owned()
        })
        .clone()
}

async fn trace(name: &str, frames: &[String]) -> Vec<String> {
    let root = tempfile::tempdir().unwrap();
    let runtime = [fake_claude().as_str(), "-p", "--input-format", "stream-json", "--trace", name]
        .map(String::from)
        .to_vec();
    let host = SessionHost::spawn(&HostConfig {
        name: "lead".into(),
        generation: 1,
        state_dir: root.path().to_path_buf(),
        socket: None,
        idle_exit: None,
        provider: None,
        runtime,
    })
    .unwrap();
    for frame in frames {
        host.stdin.enqueue(frame.clone(), None).await.unwrap();
        let sent: serde_json::Value = serde_json::from_str(frame).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let text = fs::read_to_string(root.path().join("g1/stream.jsonl")).unwrap_or_default();
                let observed = text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok());
                if observed.into_iter().any(|seen| {
                    if sent["type"] == "control_request" {
                        seen["response"]["request_id"] == sent["request_id"]
                    } else if name == "idle-interrupt" {
                        seen["type"] == "result"
                    } else {
                        seen["type"] == "command_lifecycle" && seen["command_uuid"] == sent["uuid"]
                    }
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
    }
    let code = tokio::time::timeout(Duration::from_secs(10), host.wait()).await.unwrap().unwrap();
    assert_eq!(code, 0);
    fs::read_to_string(root.path().join("g1/stream.jsonl"))
        .unwrap()
        .lines()
        .map(|line| signature(decode_frame(line).unwrap()))
        .collect()
}

fn signature(frame: StreamFrame) -> String {
    match frame {
        StreamFrame::Init(_) => "init".into(),
        StreamFrame::CommandLifecycle(c) => format!("{:?} {}", c.state, c.command_uuid),
        StreamFrame::User(u) if u.is_replay => format!("replay {}", u.uuid.unwrap_or_default()),
        StreamFrame::User(_) => "user".into(),
        StreamFrame::Assistant(a) => format!("assistant {}", a.user_message_uuids.join(",")),
        StreamFrame::Result(r) => format!(
            "result {} {}",
            r.terminal_reason.unwrap_or_default(),
            r.user_message_uuids.join(",")
        ),
        StreamFrame::TaskStarted(t) => format!("task_started {}", t.task_id),
        StreamFrame::TaskNotification(n) => format!("task {}", n.status.unwrap_or_default()),
        StreamFrame::ControlResponse(c) => format!("control {} {}", c.subtype, c.request_id),
        other => panic!("untyped frame {other:?}"),
    }
}

#[tokio::test]
async fn happy_fold_completes_both_uuids_in_one_turn() {
    let frames = trace("fold", &[encode_user_message(A, "first"), encode_user_message(B, "second")]).await;
    assert_eq!(
        frames,
        [
            format!("Queued {A}"),
            format!("AcceptedIntoTurn {A}"),
            "init".into(),
            format!("replay {A}"),
            format!("assistant {A}"),
            "task_started fake-task".into(),
            format!("Queued {B}"),
            format!("AcceptedIntoTurn {B}"),
            format!("replay {B}"),
            "task completed".into(),
            format!("assistant {B}"),
            format!("result completed {A},{B}"),
            format!("Completed {A}"),
            format!("Completed {B}"),
        ]
    );
}

#[tokio::test]
async fn interrupt_after_completion_hits_an_idle_session() {
    let frames = trace("idle-interrupt", &[encode_user_message(A, "first"), encode_interrupt("int1")]).await;
    assert_eq!(
        frames,
        [
            format!("Queued {A}"),
            format!("AcceptedIntoTurn {A}"),
            "init".into(),
            format!("replay {A}"),
            format!("assistant {A}"),
            format!("result completed {A}"),
            format!("Completed {A}"),
            "control success int1".into(),
        ]
    );
}

#[tokio::test]
async fn interrupt_cancels_the_queued_input() {
    let frames = trace(
        "cancel-queued",
        &[encode_user_message(A, "first"), encode_user_message(B, "second"), encode_interrupt("int1")],
    )
    .await;
    assert_eq!(
        frames,
        [
            format!("Queued {A}"),
            format!("AcceptedIntoTurn {A}"),
            "init".into(),
            format!("replay {A}"),
            format!("assistant {A}"),
            "task_started fake-task".into(),
            format!("Queued {B}"),
            "control success int1".into(),
            "task stopped".into(),
            "user".into(),
            "user".into(),
            format!("result aborted_tools {A}"),
            format!("Cancelled {A}"),
            format!("Cancelled {B}"),
        ]
    );
}

#[tokio::test]
async fn initialize_is_answered_with_its_request_id() {
    let frames = trace("idle-interrupt", &[encode_initialize("init1"), encode_interrupt("int1")]).await;
    assert_eq!(frames, ["control success init1", "control success int1"]);
}
