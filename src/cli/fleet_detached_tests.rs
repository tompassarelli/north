use super::*;
use crate::fleet::detached::{Detached, Scope};
use crate::fleet::waits::{Probe, Registered, Run, Waits};

struct Runs(Mutex<Option<Instant>>);

impl Scope for Runs {
    fn detached(&self) -> Vec<Detached> {
        let until = *self.0.lock().unwrap();
        let live = until.is_none_or(|until| Instant::now() < until);
        live.then(|| Detached { pid: 4242, started: 99, command: "bash capture-batch".into() }).into_iter().collect()
    }
}

async fn held(run_for: Option<Duration>, policy: crate::fleet::idle::Policy) -> (tempfile::TempDir, u8, Duration, String) {
    let root = tempfile::tempdir().unwrap();
    box_worker_record(root.path());
    let (config, stdin) = claude_turn(root.path(), 1_000, "Still waiting on the capture batch.");
    let sink = Arc::new(Recorded::default());
    let started = Instant::now();
    let scope = Arc::new(Runs(Mutex::new(run_for.map(|run| started + run))));
    let watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink, Arc::new(Held::default()), root.path().to_path_buf(), root.path().join("worker")).reading_scope(scope).idling_by(policy);
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.rest_when_idle(Some(Duration::from_secs(1)));
    let code = tokio::time::timeout(Duration::from_secs(30), host.wait()).await.unwrap().unwrap();
    let written = std::fs::read_to_string(stdin).unwrap_or_default();
    (root, code, started.elapsed(), written)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_holding_a_detached_run_neither_parks_nor_is_woken_before_it_ends_and_is_told_when_it_does() {
    let run = Duration::from_secs(5);
    let (_root, code, lasted, stdin) = held(Some(run), seconds(600, 600)).await;
    assert_eq!(code, 143);
    assert!(lasted >= run, "parked {lasted:?} into a {run:?} run past its 1 s idle limit and 1-3 s wakes");
    let first = stdin.lines().next().unwrap_or_default();
    assert!(first.contains("[north: a run you detached has ended] pid 4242 `bash capture-batch`"), "the run's end is the first thing the session hears: {stdin}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_parked_at_the_hold_registers_its_detached_run_and_the_runs_end_revives_it_naming_the_run() {
    let policy = crate::fleet::idle::Policy { detached_hold: Duration::from_secs(2), ..seconds(600, 600) };
    let (root, code, _, stdin) = held(None, policy).await;
    assert_eq!(code, 143);
    assert!(!stdin.contains("your turn ended on a wait"), "held, not woken: {stdin}");
    let waits = Waits::in_root(root.path());
    let registered = waits.of("worker");
    assert_eq!(registered.iter().map(|wait| (wait.run.clone(), wait.label.as_deref())).collect::<Vec<_>>(), [(Run::Process { pid: 4242, started: 99 }, Some("bash capture-batch"))]);

    struct Ended;
    impl Probe for Ended {
        fn result(&self, _wait: &Registered) -> Option<String> {
            Some("ended".into())
        }
    }
    let told = Mutex::new(Vec::new());
    waits.settle(&Ended, &|session, text| {
        told.lock().unwrap().push((session.to_owned(), text.to_owned()));
        Ok(())
    });
    assert_eq!(told.into_inner().unwrap(), [("worker".to_owned(), "[north: the run you waited on is over] pid:4242@99 `bash capture-batch`: ended. Read its output and continue to a final report.".to_owned())]);
    assert!(waits.of("worker").is_empty());
}
