use std::cell::RefCell;
use std::path::Path;

use super::*;

const BRIEF: &str = "Item: north#1 Thing\nBox: The thing works\nPrior report: halfway\nBudget: $5\n\nGoal: make the thing work.";

#[derive(Default)]
struct Fake {
    status: &'static str,
    sent: RefCell<Vec<(String, String)>>,
    launched: RefCell<Vec<(String, Option<String>, String)>>,
}

impl Fleet for Fake {
    fn row(&self, _: &str) -> Option<Row> {
        (!self.status.is_empty()).then(|| Row { status: self.status.to_owned(), task: Some("north#1".to_owned()), parent: Some("lead".to_owned()), brief: BRIEF.to_owned() })
    }

    fn send(&self, session: &str, text: &str) -> bool {
        self.sent.borrow_mut().push((session.to_owned(), text.to_owned()));
        true
    }

    fn launch(&self, name: &str, row: &Row, prompt: &str) -> Result<(), String> {
        self.launched.borrow_mut().push((name.to_owned(), row.task.clone(), prompt.to_owned()));
        Ok(())
    }
}

fn reverted() -> Reverted<'static> {
    Reverted { branch: "thing-lane", commit: "abcdef0123456789", revert: "sheriff-revert-abcdef012", red: "fedcba9876543210", failing: "step cargo clippy", log: Path::new("/logs/post-land-fedcba987654.log") }
}

#[test]
fn a_revert_goes_to_the_culprits_live_session_with_the_failing_step_and_log_else_a_fresh_worker_follows_the_lane_with_the_evidence() {
    let live = Fake { status: "idle", ..Fake::default() };
    tell(&live, Some("thing"), &reverted());
    let sent = live.sent.borrow();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].0, "thing");
    assert!(sent[0].1.contains("step cargo clippy") && sent[0].1.contains("/logs/post-land-fedcba987654.log") && sent[0].1.contains("Follows: abcdef012"), "{}", sent[0].1);
    assert!(live.launched.borrow().is_empty());

    let gone = Fake { status: "exited", ..Fake::default() };
    tell(&gone, Some("thing-backlog-back-1234567"), &reverted());
    let launched = gone.launched.borrow();
    assert_eq!(launched.len(), 1);
    let (name, task, prompt) = &launched[0];
    assert_eq!((name.as_str(), task.as_deref()), ("thing-backlog-back-abcdef0", Some("north#1")));
    assert!(prompt.starts_with("Item: north#1 Thing\nBox: The thing works\nFollows: thing-lane landed as abcdef0123456789"), "{prompt}");
    assert!(prompt.contains("step cargo clippy") && prompt.contains("/logs/post-land-fedcba987654.log") && prompt.contains("git cherry-pick abcdef0123456789") && prompt.ends_with("Goal: make the thing work."), "{prompt}");
    assert!(!prompt.contains("halfway") && !prompt.contains("Budget:"), "{prompt}");
    assert!(gone.sent.borrow().iter().all(|(session, _)| session == "proxy"), "only the proxy hears of a restaff: {:?}", gone.sent.borrow());

    let unknown = Fake::default();
    tell(&unknown, Some("thing"), &reverted());
    assert!(unknown.launched.borrow().is_empty());
    assert_eq!(unknown.sent.borrow()[0].0, "proxy");
}
