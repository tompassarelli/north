use std::cell::RefCell;

use super::fleet::entered;
use crate::remote_serve::{self, Grants, Job, Jobs, Launch, Plan, REMOTE_PEER, Refusal, log};

const LAPTOP_JOB: i64 = 1;
const GALILEO_JOB: i64 = 2;
const OWN_JOB: i64 = 3;

struct Ledger;

impl Jobs for Ledger {
    fn job(&self, id: i64) -> Option<Job> {
        match id {
            LAPTOP_JOB => Some(Job { placement: "laptop".into(), item: Some("north#5".into()) }),
            GALILEO_JOB => Some(Job { placement: "gex45".into(), item: Some("galileo#1".into()) }),
            OWN_JOB => Some(Job { placement: "gex45".into(), item: Some("north#5".into()) }),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Recorded(RefCell<Vec<Vec<String>>>);

impl Launch for Recorded {
    fn launch(&self, plan: &Plan) -> i32 {
        self.0.borrow_mut().push(plan.argv.clone());
        0
    }
}

fn gex45() -> Grants {
    Grants { peer: "gex45".into(), repos: vec!["north".into(), "wisp".into(), "makgora".into()], threads_only: false }
}

fn planned(command: &str) -> Result<Plan, Refusal> {
    remote_serve::plan_for(command, &gex45(), &Ledger)
}

#[test]
fn remote_serve_guards_refuse_a_peer_reaching_past_its_own_jobs_hosts_and_repositories() {
    let home = tempfile::tempdir().unwrap();
    let launched = Recorded::default();
    let mut log = log::Log::open(&home.path().join("gex45.jsonl")).unwrap();
    let refused = [
        "threads queue add laptop -- id".to_string(),
        "threads queue add pool -- id".into(),
        "threads queue take gex45,laptop".into(),
        format!("threads queue msg {LAPTOP_JOB} done x"),
        format!("threads queue drop {LAPTOP_JOB} x"),
        "threads queue msg 99 done x".into(),
        "threads queue follow north#9 --tier sonnet --parent p".into(),
        "threads queue follow-release 4".into(),
        "threads agent-event x".into(),
        "threads incident import".into(),
        "threads incident report".into(),
        "threads incident".into(),
        "threads incident record --key k --class c --noticed-by w --repo galileo".into(),
        "threads incident record --key k --class c --noticed-by w --repo".into(),
        "threads incident record --key k --key j".into(),
        "threads incident record --guard x".into(),
        "threads agent-event-state 1 x".into(),
        format!("threads queue msg {GALILEO_JOB} failed x"),
        "threads block south#1 north#2".into(),
        "threads claim galileo#3".into(),
        "threads new south title".into(),
        "threads job-run nightly --host whiterabbit --started s".into(),
        "threads job-run nightly --host gex45 --host laptop --started s".into(),
        "threads job-run nightly --started s".into(),
        "north fleet row put --peer laptop".into(),
        "north fleet row exchange extra".into(),
    ];
    for command in &refused {
        assert_eq!(remote_serve::serve(Some(command), &gex45(), &Ledger, &launched, &mut log, &mut Vec::new()).unwrap(), 126, "{command}");
    }
    assert!(launched.0.borrow().is_empty());
    let rows = log::read(&home.path().join("gex45.jsonl")).unwrap().rows;
    assert_eq!(rows.len(), refused.len());
    assert!(rows.iter().all(|row| row.refusal.is_some() && row.exit == 126));
}

#[test]
fn remote_serve_forces_the_peer_on_every_fleet_row_verb() {
    for verb in ["put", "exchange", "register", "wait", "settle", "retire"] {
        for command in [format!("north fleet row {verb}"), format!("north fleet row {verb} --peer gex45")] {
            let plan = planned(&command).unwrap();
            assert_eq!(plan.argv, ["north", "fleet", "row", verb, "--peer", "gex45"], "{command}");
            assert_eq!(plan.env[REMOTE_PEER], "gex45");
            assert_eq!(entered(Some("gex45"), |name| plan.env.get(name).cloned()).unwrap(), Some("gex45"));
            assert!(entered(Some("laptop"), |name| plan.env.get(name).cloned()).is_err());
        }
        assert!(matches!(planned(&format!("north fleet row {verb} --peer laptop")), Err(Refusal::Guard(_))));
    }
    assert_eq!(planned("north fleet usage --refresh --local --json").unwrap().verb, "fleet usage");
    assert!(matches!(planned("agents usage --refresh --local --json"), Err(Refusal::Verb(_))));
    assert_eq!(planned("north github-mirror serve").unwrap().verb, "github-mirror serve");
    assert!(matches!(planned("north github-mirror sync"), Err(Refusal::Verb(_))));
    assert_eq!(planned("north fleet row leased w1").unwrap().argv, ["north", "fleet", "row", "leased", "w1"]);
    assert_eq!(planned("north fleet dispatch needs north#775").unwrap().argv, ["north", "fleet", "dispatch", "needs", "north#775"]);
    assert_eq!(planned("north fleet dispatch need n-x gex45 north#791").unwrap().argv, ["north", "fleet", "dispatch", "need", "n-x", "gex45", "north#791"]);
}


#[test]
fn remote_serve_admits_incident_record_from_a_peer_for_a_granted_repository() {
    for command in [
        "north threads incident record --key k --class c --noticed-by lander --repo north --subject b --started-at 1 --detected-at 2 --detail 'a b'",
        "threads incident record --key k --class c --noticed-by lease-reaper --subject gex45 --detail d",
    ] {
        assert!(planned(command).is_ok(), "{command}");
    }
}
