use super::*;

struct Ahead(u32);

impl crate::fleet::stranded::Lanes for Ahead {
    fn checkout(&self, _machine: Option<&crate::fleet::model::Machine>, _root: &Path) -> Option<crate::fleet::stranded::Checkout> {
        Some(crate::fleet::stranded::Checkout { worktrees: vec![crate::fleet::stranded::Worktree { path: "/tmp".into(), branch: Some("g1".into()) }] })
    }

    fn lanes(&self, _machine: Option<&crate::fleet::model::Machine>, trees: &[crate::fleet::stranded::Worktree]) -> Vec<crate::fleet::stranded::Lane> {
        trees.iter().map(|tree| crate::fleet::stranded::Lane { path: tree.path.clone(), branch: tree.branch.clone(), ahead: self.0, dirty: false, landed: false }).collect()
    }
}

async fn silent_run(reply: &str, lanes: Arc<dyn crate::fleet::stranded::Lanes + Send + Sync>, policy: crate::fleet::idle::Policy, park_after: Duration) -> Vec<(String, String, Delivery)> {
    silent_run_as(reply, lanes, policy, park_after, json!({})).await
}

async fn silent_run_as(reply: &str, lanes: Arc<dyn crate::fleet::stranded::Lanes + Send + Sync>, policy: crate::fleet::idle::Policy, park_after: Duration, extra: serde_json::Value) -> Vec<(String, String, Delivery)> {
    silent_run_with_jobs(reply, lanes, policy, park_after, extra, Arc::new(crate::fleet::jobs::HeldLeases::default())).await
}

async fn silent_run_with_jobs(reply: &str, lanes: Arc<dyn crate::fleet::stranded::Lanes + Send + Sync>, policy: crate::fleet::idle::Policy, park_after: Duration, extra: serde_json::Value, jobs: Arc<dyn crate::fleet::jobs::JobLeases>) -> Vec<(String, String, Delivery)> {
    let root = tempfile::tempdir().unwrap();
    session_record(root.path(), Some("lead"));
    let record = root.path().join("worker/session.json");
    let mut fields: serde_json::Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    fields.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    std::fs::write(&record, fields.to_string()).unwrap();
    let (config, _) = claude_turn(root.path(), 1_000, reply);
    let sink = Arc::new(Recorded::default());
    let watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink.clone(), Arc::new(Held::default()), root.path().to_path_buf(), root.path().join("worker")).reading_lanes(lanes).reading_jobs(jobs).idling_by(policy);
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.rest_when_idle(Some(park_after));
    tokio::time::timeout(Duration::from_secs(30), host.wait()).await.unwrap().unwrap();
    let posted = sink.1.lock().unwrap().clone();
    posted.into_iter().filter(|(_, text, _)| text.contains("north fleet show worker")).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_that_parks_with_no_report_line_tells_its_parent_once_which_state_it_is_in() {
    let told = silent_run("Landing now.", Arc::new(Ahead(0)), seconds(600, 600), Duration::from_secs(2)).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!((told[0].0.as_str(), told[0].2), ("lead", Delivery::Queued));
    assert!(told[0].1.starts_with("[from worker · parked · turn 1] its last turn ended with no Done:"), "{}", told[0].1);
    assert!(!told[0].1.contains("commit"), "no lane, no ahead count: {}", told[0].1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_idle_past_the_silent_limit_is_told_as_idle_before_it_parks_and_not_told_again_when_it_does() {
    let policy = crate::fleet::idle::Policy { eta_minute: Duration::from_millis(11), ..seconds(600, 600) };
    let told = silent_run("Landing now.", Arc::new(Ahead(3)), policy, Duration::from_secs(4)).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].1.starts_with("[from worker · idle · turn 1]") && told[0].1.contains("its lane g1 holds 3 commits not on origin/main"), "{}", told[0].1);
}
