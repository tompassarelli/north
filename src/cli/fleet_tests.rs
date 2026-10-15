use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::fleet::registry::TurnWatch;
use crate::fleet::turns::{RunRecord, TurnExit};
use crate::session_host::{SessionHost, Watcher};

#[derive(Default)]
struct Recorded(Mutex<Vec<RunRecord>>);

impl RunSink for Recorded {
    fn record_run(&self, run: &RunRecord) -> NorthResult<()> {
        self.0.lock().unwrap().push(run.clone());
        Ok(())
    }

    fn join_task(&self, _task_ref: &str, _session: &str) -> NorthResult<()> {
        Ok(())
    }
}

fn silent_codex_turn(root: &Path, silent_secs: u64) -> HostConfig {
    let thread = r#""threadId":"th-1""#;
    let script = format!(
        r#"echo '{{"method":"turn/started","params":{{{thread},"turn":{{"id":"turn-1"}}}}}}'; sleep {silent_secs}; echo '{{"method":"turn/completed","params":{{{thread},"turn":{{"id":"turn-1","status":"completed","durationMs":{}}}}}}}'; exec sleep 60"#,
        silent_secs * 1000
    );
    HostConfig {
        name: "codex-test".into(),
        generation: 1,
        state_dir: root.to_path_buf(),
        socket: None,
        idle_exit: None,
        provider: Some("codex".into()),
        runtime: vec!["sh".into(), "-c".into(), script],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_codex_turn_is_never_parked_mid_turn() {
    let root = tempfile::tempdir().unwrap();
    let config = silent_codex_turn(root.path(), 3);
    let sink = Arc::new(Recorded::default());
    let watch = TurnWatch::new(Arc::new(Codex), sink.clone(), root.path().to_path_buf(), root.path().to_path_buf());
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    let started = Instant::now();
    host.park_when_idle(Duration::from_secs(1));
    let code = tokio::time::timeout(Duration::from_secs(20), host.wait()).await.unwrap().unwrap();
    assert_eq!(code, 143, "the host parks by SIGTERM once the turn has ended");
    assert!(started.elapsed() > Duration::from_secs(3), "parked after {:?}, inside the 3 s silent turn", started.elapsed());
    let runs = sink.0.lock().unwrap();
    assert_eq!(runs.iter().map(|run| (run.turn.exit, run.turn.wall_ms)).collect::<Vec<_>>(), [(TurnExit::Completed, 3000)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifications_after_the_turn_do_not_keep_a_codex_host_awake() {
    let root = tempfile::tempdir().unwrap();
    let thread = r#""threadId":"th-1""#;
    let script = format!(
        r#"echo '{{"method":"turn/started","params":{{{thread},"turn":{{"id":"turn-1"}}}}}}'; echo '{{"method":"turn/completed","params":{{{thread},"turn":{{"id":"turn-1","status":"completed"}}}}}}'; while :; do echo '{{"method":"skills/changed","params":{{}}}}'; sleep 0.2; done"#
    );
    let mut config = silent_codex_turn(root.path(), 0);
    config.runtime = vec!["sh".into(), "-c".into(), script];
    let watch = TurnWatch::new(Arc::new(Codex), Arc::new(Recorded::default()), root.path().to_path_buf(), root.path().to_path_buf());
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.park_when_idle(Duration::from_secs(1));
    let parked = tokio::time::timeout(Duration::from_secs(10), host.wait()).await;
    assert_eq!(parked.ok().map(Result::unwrap), Some(143), "an idle host must park despite steady non-turn output");
}
