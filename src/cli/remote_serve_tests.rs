use std::cell::RefCell;

use crate::remote_serve::{self, Grants, Launch, Plan, Refusal, log};

#[derive(Default)]
struct Recorded(RefCell<Vec<Vec<String>>>);

impl Launch for Recorded {
    fn launch(&self, plan: &Plan) -> i32 {
        self.0.borrow_mut().push(plan.argv.clone());
        0
    }
}

fn gex45() -> Grants {
    Grants { peer: "gex45".into() }
}

fn planned(command: &str) -> Result<Plan, Refusal> {
    remote_serve::plan_for(command, &gex45())
}

#[test]
fn remote_serve_logs_and_refuses_evicted_and_unlisted_peer_commands() {
    let home = tempfile::tempdir().unwrap();
    let launched = Recorded::default();
    let mut log = log::Log::open(&home.path().join("gex45.jsonl")).unwrap();
    let refused = [
        "threads list",
        "north threads show north#1",
        "env THREADS_ACTOR=w1 threads claim north#1",
        "north fleet row put --peer gex45",
        "north github-mirror serve",
        "south land extra",
    ];
    for command in refused {
        assert_eq!(remote_serve::serve(Some(command), &gex45(), &launched, &mut log, &mut Vec::new()).unwrap(), 126, "{command}");
    }
    assert!(launched.0.borrow().is_empty());
    let rows = log::read(&home.path().join("gex45.jsonl")).unwrap().rows;
    assert_eq!(rows.len(), refused.len());
    assert!(rows.iter().all(|row| row.refusal.is_some() && row.exit == 126));
    assert!(super::remote_serve::options(&["--peer".into(), "gex45".into(), "--threads-only".into()]).is_err());
    assert!(super::remote_serve::options(&["--peer".into(), "gex45".into(), "--repos".into(), "north,south".into()]).is_ok());
}

#[test]
fn remote_serve_admits_only_the_kept_peer_verbs() {
    assert_eq!(planned("north runtime health").unwrap().verb, "runtime health");
    assert_eq!(planned("north holds list --here --json").unwrap().verb, "holds list");
    assert_eq!(planned("readlink .local/state/north/current").unwrap().verb, "readlink current");
    assert_eq!(planned("south land").unwrap().verb, "south land");
    assert!(matches!(planned("agents usage --refresh --local --json"), Err(Refusal::Verb(_))));
    assert!(matches!(planned("north github-mirror sync"), Err(Refusal::Verb(_))));
}
