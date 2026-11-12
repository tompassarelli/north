use super::*;

const EVENTS: &str = include_str!("../fixtures/cloud/handback-events.jsonl");

fn events() -> Vec<Value> {
    EVENTS.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_reaped_cloud_lane_queues_for_the_fleet_session_that_launched_it() {
    let root = tempfile::tempdir().unwrap();
    let launch = CloudLaunch { at: 0.0, repo: "north".into(), session: "session_01lead".into(), branch: "claude/land-lead".into(), owner: Some("lead-fleet".into()) };
    assert_eq!(queuing_session(Some("burn-7".into()), Some(&launch)).as_deref(), Some("burn-7"));
    let queued_for = queuing_session(None, Some(&launch));
    let common = format!("{}/north/.git\n", root.path().display());
    let mut pushed_for = Vec::new();
    let mut run = |_: &Path, program: &str, args: &[&str], session: Option<&str>| -> Result<String, String> {
        match (program, args.first().copied()) {
            ("git", Some("rev-parse")) if args.contains(&"--git-common-dir") => Ok(common.clone()),
            ("git", Some("rev-parse")) => Ok("0123456789abcdef0123456789abcdef01234567\n".into()),
            ("git", Some("log")) => Ok("Land the handback\0".into()),
            ("safe-push", _) => {
                pushed_for.push(session.map(str::to_owned));
                Ok("Done: queued".into())
            }
            _ => Ok(String::new()),
        }
    };
    let outcome = land_handback(&events(), &root.path().join("events.jsonl"), root.path(), &root.path().join("state"), queued_for.as_deref(), &mut run);
    assert!(matches!(outcome, Some(Outcome::Queued { .. })), "{outcome:?}");
    assert_eq!(pushed_for, [Some("lead-fleet".to_owned())]);
}
