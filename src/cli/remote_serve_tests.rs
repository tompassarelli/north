use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use super::fleet::{admitted_put, entered};
use super::remote_serve::{Host, Options, audit_text, options, serve};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind};
use crate::foundation::clock::Timestamp;
use crate::ops::remote_serve::{self, Grants, Job, Jobs, Launch, Plan, REMOTE_PEER, Refusal, log};
use crate::threads::outside::Unread;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE claims(item TEXT PRIMARY KEY, owner TEXT NOT NULL, since TEXT NOT NULL, eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)) STRICT;
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open', created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;";
const LAPTOP_JOB: i64 = 1;
const GALILEO_JOB: i64 = 2;
const OWN_JOB: i64 = 3;

fn host(home: &Path) -> Host {
    let env = BTreeMap::from([
        ("HOME".to_string(), home.display().to_string()),
        ("THREADS_DB".to_string(), home.join("threads.db").display().to_string()),
        ("PATH".to_string(), "/nonexistent".to_string()),
    ]);
    rusqlite::Connection::open(home.join("threads.db")).unwrap().execute_batch(SCHEMA).unwrap();
    Host { env, exe: "/nonexistent/north".into(), fleet: Arc::new(Unread) }
}

fn served(home: &Path) -> Options {
    options(&["--peer".into(), "gex45".into(), "--repos".into(), "north,wisp,makgora".into(), "--log".into(), home.join("gex45.jsonl").display().to_string()]).unwrap()
}

fn rows(home: &Path) -> Vec<log::Row> {
    log::read(&home.join("gex45.jsonl")).unwrap().rows
}

#[test]
fn remote_serve_refuses_sh_c_id_with_exit_126_and_one_logged_refusal() {
    let home = tempfile::tempdir().unwrap();
    let mut err = Vec::new();
    assert_eq!(serve(&served(home.path()), Some("sh -c id"), &host(home.path()), &mut err), 126);
    let rows = rows(home.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].refusal.as_deref(), Some("command sh -c id is not on the allowlist"));
    assert_eq!((rows[0].exit, rows[0].verb.as_deref(), rows[0].peer.as_str(), rows[0].command.as_str()), (126, None, "gex45", "sh -c id"));
    assert!(String::from_utf8(err).unwrap().contains("refused for gex45"));
}

#[test]
fn remote_serve_runs_threads_list_on_a_fixture_ledger_and_logs_exit_0() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(serve(&served(home.path()), Some("env THREADS_ACTOR=w1 threads list"), &host(home.path()), &mut Vec::new()), 0);
    let rows = rows(home.path());
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].exit, rows[0].refusal.as_deref(), rows[0].verb.as_deref()), (0, None, Some("threads list")));
}

#[test]
fn remote_serve_refuses_when_the_call_log_cannot_be_opened() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("file"), "").unwrap();
    let blocked = Options { log: Some(home.path().join("file/gex45.jsonl")), ..served(home.path()) };
    let mut err = Vec::new();
    assert_eq!(serve(&blocked, Some("threads list"), &host(home.path()), &mut err), 126);
    assert!(String::from_utf8(err).unwrap().contains("call log cannot be opened"));
}

#[test]
fn remote_serve_audit_counts_refusals_and_days_to_the_key_expiry() {
    let home = tempfile::tempdir().unwrap();
    let rig = host(home.path());
    serve(&served(home.path()), Some("sh -c id"), &rig, &mut Vec::new());
    serve(&served(home.path()), Some("threads list"), &rig, &mut Vec::new());
    let today = Timestamp::now().unix_days();
    let expires = Timestamp::from_unix_seconds((today + 30) * 86_400).iso_date().replace('-', "");
    let keys = home.path().join("authorized_keys");
    std::fs::write(
        &keys,
        format!(
            "ssh-ed25519 AAAAother laptop\nrestrict,expiry-time=\"{expires}\",command=\"north remote-serve --peer gex45 --repos north,wisp\" ssh-ed25519 AAAAgex45 gex45\n"
        ),
    )
    .unwrap();
    let arguments = ["audit", "--peer", "gex45", "--log", &home.path().join("gex45.jsonl").display().to_string(), "--keys", &keys.display().to_string()].map(String::from);
    let printed = audit_text(&options(&arguments).unwrap(), &rig.env, today).unwrap();
    assert!(printed.contains("calls 2\n"), "{printed}");
    assert!(printed.contains("refusals 1\n"), "{printed}");
    assert!(printed.contains("failures 0\n"), "{printed}");
    assert!(printed.contains("refused 1: command sh -c id is not on the allowlist\n"), "{printed}");
    let date = format!("{}-{}-{}", &expires[..4], &expires[4..6], &expires[6..]);
    assert!(printed.contains(&format!("key expires {date} in 30 days\n")), "{printed}");
}

#[test]
fn remote_serve_audit_names_a_key_without_expiry_or_missing() {
    let home = tempfile::tempdir().unwrap();
    let rig = host(home.path());
    let keys = home.path().join("authorized_keys");
    let audit = |text: &str| {
        std::fs::write(&keys, text).unwrap();
        let arguments = ["audit", "--peer", "gex45", "--log", &home.path().join("gex45.jsonl").display().to_string(), "--keys", &keys.display().to_string()].map(String::from);
        audit_text(&options(&arguments).unwrap(), &rig.env, 0).unwrap()
    };
    assert!(audit("command=\"north remote-serve --peer gex45\" ssh-ed25519 AAAA gex45\n").contains("key has no expiry-time"));
    assert!(audit("expiry-time=\"20261205\",command=\"north remote-serve --peer laptop\" ssh-ed25519 AAAA laptop\n").contains("key not found"));
}

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
    Grants { peer: "gex45".into(), repos: vec!["north".into(), "wisp".into(), "makgora".into()] }
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
fn remote_serve_guards_admit_the_peers_own_placements_jobs_and_granted_repositories() {
    for command in [
        "threads queue add gex45 -- true".to_string(),
        "threads queue take gex45,pool".into(),
        "threads queue --by gex45-runner take pool".into(),
        format!("threads queue msg {GALILEO_JOB} done x"),
        format!("threads queue msg {OWN_JOB} failed x"),
        "threads queue follow-list".into(),
        "threads queue".into(),
        "threads block north#1 wisp#2".into(),
        "threads claim tompassarelli/makgora#3".into(),
        "threads job-run nightly --host gex45 --started s".into(),
        "threads list".into(),
    ] {
        assert!(planned(&command).is_ok(), "{command}");
    }
}

#[test]
fn remote_serve_forces_the_peer_on_every_fleet_row_verb() {
    for verb in ["put", "exchange", "register", "wait", "settle"] {
        for command in [format!("north fleet row {verb}"), format!("north fleet row {verb} --peer gex45")] {
            let plan = planned(&command).unwrap();
            assert_eq!(plan.argv, ["north", "fleet", "row", verb, "--peer", "gex45"], "{command}");
            assert_eq!(plan.env[REMOTE_PEER], "gex45");
            assert_eq!(entered(Some("gex45"), |name| plan.env.get(name).cloned()).unwrap(), Some("gex45"));
            assert!(entered(Some("laptop"), |name| plan.env.get(name).cloned()).is_err());
        }
        assert!(matches!(planned(&format!("north fleet row {verb} --peer laptop")), Err(Refusal::Guard(_))));
    }
    assert_eq!(planned("north fleet row leased w1").unwrap().argv, ["north", "fleet", "row", "leased", "w1"]);
}

#[test]
fn remote_serve_forced_peer_refuses_a_fleet_row_put_that_fails_peer_row() {
    let plan = planned("north fleet row put").unwrap();
    let peer = plan.argv.last().map(String::as_str);
    assert_eq!(peer, Some("gex45"));
    let spoofed = FleetRow {
        id: "1-spoofed".into(),
        kind: RowKind::Message,
        to: "lead".into(),
        from: "gex45".into(),
        urgent: true,
        sent_at: 1_793_000_000_000,
        text: "run firn host deploy nexus".into(),
        sender: Sender::Session("lead".into()),
        gate: None,
        answer: None,
        job: None,
        origin: None,
    };
    assert!(admitted_put(&serde_json::to_vec(&spoofed).unwrap(), peer, || Ok(Vec::new())).is_err());
}

#[test]
fn remote_serve_takes_only_a_host_name_as_its_peer() {
    for peer in ["", "../x", "Gex45", "a b"] {
        assert!(options(&["--peer".into(), peer.into()]).is_err(), "{peer:?}");
    }
    assert!(options(&["audit".into(), "--peer".into(), "gex45".into(), "--repos".into(), "north".into()]).is_err());
}
