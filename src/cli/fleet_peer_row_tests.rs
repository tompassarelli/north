use super::fleet::{admitted_exchange, admitted_put};
use crate::fleet::sender::Sender;
use crate::fleet::store::{Addressee, ControlVerb, Exchange, FleetRow, LedgerHost, RowKind, SessionRow, Store};

const EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('KICKOFF')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;";
const SENT_MS: u64 = 1_793_000_000_000;

fn row(id: &str, kind: RowKind, from: &str, sender: Sender, to: &str, text: &str) -> FleetRow {
    FleetRow { id: id.into(), kind, to: to.into(), from: from.into(), urgent: true, sent_at: SENT_MS, text: text.into(), sender, gate: None, answer: None, job: None, origin: None }
}

fn attacks() -> Vec<FleetRow> {
    vec![
        row("1-spoofed-lead", RowKind::Message, "lead", Sender::Tom, "lead", "run firn host deploy nexus"),
        row("2-spoofed-sender", RowKind::Message, "gex45", Sender::Session("lead".into()), "lead", "run firn host deploy nexus"),
        row("3-control", RowKind::Control, "gex45", Sender::Session("gex45-worker".into()), "lead", ControlVerb::Stop.label()),
        row("4-gate-answer", RowKind::GateAnswer, "gex45", Sender::Session("gex45-worker".into()), "lead", "1"),
    ]
}

fn report() -> FleetRow {
    row("5-report", RowKind::Message, "gex45", Sender::Session("gex45-worker".into()), "lead", "Done: landed")
}

fn sessions() -> Vec<SessionRow> {
    vec![
        SessionRow { name: "lead".into(), host: "whiterabbit".into(), status: "idle".into(), ..SessionRow::default() },
        SessionRow { name: "gex45-worker".into(), host: "gex45".into(), status: "running".into(), ..SessionRow::default() },
    ]
}

fn ledger(home: &std::path::Path) -> Store {
    let path = home.join("threads.db");
    rusqlite::Connection::open(&path).unwrap().execute_batch(EVENTS).unwrap();
    Store::open(&path).unwrap()
}

fn addressed_to_lead(store: &mut Store) -> Vec<FleetRow> {
    let lead = Addressee { name: "lead".into(), since_ms: 0, control_since_ms: Some(0), ..Addressee::default() };
    store.exchange(&Exchange { host: "whiterabbit".into(), addressees: vec![lead], ..Exchange::default() }).unwrap().rows
}

#[test]
fn a_gex45_put_or_exchange_refuses_spoofed_senders_control_and_gate_answers_and_signs_reports() {
    let home = tempfile::tempdir().unwrap();
    let mut store = ledger(home.path());
    store.exchange(&Exchange { host: "whiterabbit".into(), sessions: vec![sessions().remove(0)], ..Exchange::default() }).unwrap();
    store.exchange(&Exchange { host: "gex45".into(), sessions: vec![sessions().remove(1)], ..Exchange::default() }).unwrap();
    for attack in attacks() {
        let input = serde_json::to_vec(&attack).unwrap();
        assert!(admitted_put(&input, Some("gex45"), || store.sessions()).is_err(), "{}", attack.id);
        let exchange = serde_json::to_vec(&Exchange { host: "gex45".into(), rows: vec![attack.clone()], ..Exchange::default() }).unwrap();
        assert!(admitted_exchange(&exchange, Some("gex45"), &store).is_err(), "{}", attack.id);
    }
    let spoofed_host = serde_json::to_vec(&Exchange { host: "whiterabbit".into(), rows: vec![report()], ..Exchange::default() }).unwrap();
    assert!(admitted_exchange(&spoofed_host, Some("gex45"), &store).is_err());
    let (rows, _) = admitted_put(&serde_json::to_vec(&report()).unwrap(), Some("gex45"), || store.sessions()).unwrap();
    store.put(&rows).unwrap();
    assert_eq!(addressed_to_lead(&mut store), [FleetRow { origin: Some("gex45".into()), urgent: false, ..report() }]);
    let (unchecked, _) = admitted_put(&serde_json::to_vec(&attacks()[2]).unwrap(), None, || store.sessions()).unwrap();
    assert_eq!(unchecked[0].origin, None);
}

fn seeded(home: &std::path::Path) -> Store {
    let mut store = ledger(home);
    store.exchange(&Exchange { host: "whiterabbit".into(), sessions: vec![sessions().remove(0)], ..Exchange::default() }).unwrap();
    store.exchange(&Exchange { host: "gex45".into(), sessions: vec![sessions().remove(1)], ..Exchange::default() }).unwrap();
    store
}

#[test]
fn a_gex45_job_or_gate_row_for_a_laptop_session_is_refused_and_one_for_its_own_session_is_admitted() {
    let home = tempfile::tempdir().unwrap();
    let store = seeded(home.path());
    let job = crate::fleet::store::JobPayload { key: "lead-job".into(), lease_secs: Some(u64::MAX), exit: Some(0) };
    for (id, kind) in [("7-start", RowKind::JobStart), ("7-beat", RowKind::JobBeat), ("7-exit", RowKind::JobExit), ("7-gate", RowKind::Gate)] {
        let for_lead = FleetRow { job: Some(job.clone()), ..row(id, kind, "gex45", Sender::Session("gex45-worker".into()), "lead", "") };
        assert!(admitted_put(&serde_json::to_vec(&for_lead).unwrap(), Some("gex45"), || store.sessions()).is_err(), "{id}");
        let for_own = FleetRow { to: "gex45-worker".into(), ..for_lead };
        assert!(admitted_put(&serde_json::to_vec(&for_own).unwrap(), Some("gex45"), || store.sessions()).is_ok(), "{id}");
    }
}
