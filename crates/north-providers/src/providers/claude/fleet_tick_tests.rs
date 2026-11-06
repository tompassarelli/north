use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::fleet::tick::*;
use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::{Fleet, HandedIn};
use crate::fleet::store::{Exchange, LedgerHost};
use crate::fleet::runtimes::Runtimes;
use crate::fleet::sender::Sender;
use crate::fleet::store::{Exchanged, FarRegistry, FarSpools, FleetRow, FleetTable, HandedRow, Pull, RowKind, Store, Undecodable, UnknownKind, WaitKey};
use crate::fleet::fakes::Paper;
use super::Claude;

const EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('KICKOFF')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;";
const CREATED_MS: u64 = 1_793_000_000_000;
const NOW_MS: u64 = CREATED_MS + 60_000;
const BUDGET: Budgets = Budgets::each(Duration::from_secs(20));

struct Host {
    home: tempfile::TempDir,
    fleet: Fleet,
    ledger: PathBuf,
}

fn host(name: &str) -> Host {
    let home = tempfile::tempdir().unwrap();
    let books = Arc::new(Paper::default());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), books).unwrap().hosted_as(name).arming(|_| Ok(()));
    let ledger = home.path().join("threads.db");
    rusqlite::Connection::open(&ledger).unwrap().execute_batch(EVENTS).unwrap();
    Host { home, fleet, ledger }
}

impl Host {
    fn root(&self) -> PathBuf {
        self.home.path().join(".local/state/north/fleet")
    }

    fn store(&self) -> Store {
        Store::open(&self.ledger).unwrap()
    }

    fn opened(&self) -> OpenLedger {
        let path = self.ledger.clone();
        open_ledger(move || Store::open(&path))
    }

    fn pulls(&self) -> Pulls {
        Pulls { spools: Arc::new(Nobody), always: Vec::new() }
    }

    fn tick(&self) -> Ticked {
        tick(&self.fleet, &self.opened(), &self.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap()
    }
}

fn live(host: &Host, name: &str) -> Arc<Mutex<Vec<String>>> {
    live_with(host, name, "Not done: waiting.", None)
}

fn live_with(host: &Host, name: &str, reply: &str, task_ref: Option<&str>) -> Arc<Mutex<Vec<String>>> {
    let dir = host.root().join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let mut record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": CREATED_MS, "host_pid": std::process::id(), "conversation": name, "host_dir": dir, "stream_offset": 0});
    record["task_ref"] = json!(task_ref);
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let assistant = json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": reply}]}});
    let result = json!({"type": "result", "subtype": "success", "session_id": name});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
    let listener = UnixListener::bind(dir.join("host.sock")).unwrap();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let seen = frames.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut writer = &stream;
            for line in BufReader::new(&stream).lines().map_while(Result::ok) {
                seen.lock().unwrap().push(line);
                writeln!(writer, "{{\"host_seq\":1}}").unwrap();
            }
        }
    });
    frames
}

fn message(id: &str, to: &str, sent_at: u64) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at, text: "hello".into(), sender: Sender::Session("lead".into()), gate: None, answer: None, job: None, origin: None }
}

fn far_message(id: &str, machine: &str) -> FleetRow {
    FleetRow { from: machine.into(), sender: Sender::Tom, ..message(id, "elsewhere", NOW_MS) }
}

fn count(ledger: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(ledger).unwrap().query_row(sql, [], |row| row.get(0)).unwrap()
}

struct Unreachable;

impl LedgerHost for Unreachable {
    fn exchange(&mut self, _: &Exchange) -> NorthResult<Exchanged> {
        Err(NorthError::Refused("ssh nexus: timed out; remote outcome unknown".into()))
    }

    fn settle(&mut self, _: &[WaitKey]) -> NorthResult<()> {
        Err(NorthError::Refused("ssh nexus: timed out; remote outcome unknown".into()))
    }
}

#[test]
fn a_row_already_in_the_sessions_journal_is_not_delivered_again_by_a_second_tick() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("m-1".to_owned(), "worker".to_owned())]);
    assert_eq!((second.delivered.len(), second.already), (0, 0), "a journaled id is not fetched again");
    assert_eq!(host.fleet.hand_in(&message("m-1", "worker", NOW_MS)).unwrap(), HandedIn::Already);
    assert_eq!(frames.lock().unwrap().len(), 1);
    assert!(frames.lock().unwrap()[0].contains("[from lead@whiterabbit] hello"), "a row from another host is signed with its host");
}

struct Nobody;

impl FarSpools for Nobody {
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn drop(&self, ssh: &str, _: &[String]) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn set_aside(&self, ssh: &str, _: &[String]) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn cache(&self, ssh: &str, _: &FleetTable) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn deliver(&self, ssh: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }
}

#[derive(Default)]
struct Far {
    spools: Mutex<BTreeMap<String, Vec<FleetRow>>>,
    down: Mutex<Vec<String>>,
    undroppable: Mutex<Vec<String>>,
    undecodable: Mutex<BTreeMap<String, Vec<String>>>,
    newer: Mutex<BTreeMap<String, Vec<String>>>,
    taken: Mutex<Vec<String>>,
    aside: Mutex<Vec<String>>,
    cached: Mutex<BTreeMap<String, Vec<String>>>,
}

impl Far {
    fn holding(ssh: &str, rows: Vec<FleetRow>) -> Self {
        let far = Self::default();
        far.spools.lock().unwrap().insert(ssh.into(), rows);
        far
    }

    fn left_on(&self, ssh: &str) -> Vec<String> {
        self.spools.lock().unwrap().get(ssh).map(|rows| rows.iter().map(|row| row.id.clone()).collect()).unwrap_or_default()
    }
}

impl FarSpools for Far {
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        self.taken.lock().unwrap().push(ssh.into());
        if self.down.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection timed out")));
        }
        let rows = self.spools.lock().unwrap().get(ssh).cloned().unwrap_or_default();
        let undecodable = self.undecodable.lock().unwrap().get(ssh).into_iter().flatten().map(|id| Undecodable { id: id.clone(), why: "this build cannot read it: missing field `id`".into() }).collect();
        let unknown_kind = self.newer.lock().unwrap().get(ssh).into_iter().flatten().map(|id| UnknownKind { id: id.clone(), kind: "ANSWER-V2".into() }).collect();
        Ok(Pull { rows, undecodable, unknown_kind })
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        if self.undroppable.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection reset")));
        }
        self.spools.lock().unwrap().entry(ssh.into()).or_default().retain(|row| !ids.contains(&row.id));
        self.undecodable.lock().unwrap().entry(ssh.into()).or_default().retain(|id| !ids.contains(id));
        Ok(())
    }

    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        self.aside.lock().unwrap().extend(ids.iter().cloned());
        self.drop(ssh, ids)
    }

    fn cache(&self, ssh: &str, table: &FleetTable) -> NorthResult<()> {
        self.cached.lock().unwrap().insert(ssh.into(), table.sessions.iter().map(|session| session.name.clone()).collect());
        Ok(())
    }

    fn registry(&self, _: &str) -> NorthResult<FarRegistry> {
        Ok(FarRegistry::default())
    }

    fn deliver(&self, _: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(Vec::new())
    }
}

fn far_host(name: &str) -> FarHost {
    FarHost { name: name.into(), ssh: format!("tom@{name}") }
}

#[test]
fn a_row_pulled_from_a_far_spool_is_dropped_there_only_after_the_ledger_host_commits_it() {
    let host = host("nexus");
    let far = Arc::new(Far::holding("tom@gex45", vec![far_message("m-far", "gex45")]));
    let pulls = Pulls { spools: far.clone(), always: vec![far_host("gex45")] };

    assert!(tick(&host.fleet, &open_ledger(|| Ok(Unreachable)), &pulls, &NoProbes, NOW_MS, BUDGET).is_err());
    assert_eq!(far.left_on("tom@gex45"), ["m-far"], "a row whose commit is unknown stays on its machine");

    let ticked = tick(&host.fleet, &host.opened(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.pulled, 1);
    assert!(ticked.pull_failed.is_empty());
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-far'"), 1);
}

#[test]
fn a_row_this_build_cannot_decode_is_set_aside_on_its_machine_with_a_notice_and_the_other_rows_are_still_delivered() {
    let host = host("nexus");
    let far = Arc::new(Far::holding("tom@gex45", vec![far_message("m-ok", "gex45")]));
    far.undecodable.lock().unwrap().insert("tom@gex45".into(), vec!["m-old-answer".into()]);
    let pulls = Pulls { spools: far.clone(), always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &host.opened(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert!(ticked.pull_failed.is_empty(), "{ticked:?}");
    assert_eq!(*far.aside.lock().unwrap(), ["m-old-answer"]);
    assert!(far.undecodable.lock().unwrap()["tom@gex45"].is_empty() && far.left_on("tom@gex45").is_empty());
    assert!(ticked.lines().iter().any(|line| line.starts_with("set aside on gex45, the store refuses it: m-old-answer: this build cannot read it")), "{:?}", ticked.lines());
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-ok'"), 1);
}

struct Table(crate::fleet::handover::Observed);

impl crate::fleet::handover::AccountReadings for Table {
    fn observed(&self, _: u64) -> crate::fleet::handover::Observed {
        crate::fleet::handover::Observed { readings: self.0.readings.clone(), limits: self.0.limits.clone() }
    }
}

fn read(row: i64, account: &str, window: crate::usage::decide::world::Window, used: f64, resets_in_s: i64) -> crate::usage::decide::world::Reading {
    use crate::usage::decide::world::{Label, Percent, Reading, RowId, Source, Unix};
    let now = (NOW_MS / 1000) as i64;
    Reading { row: RowId(row), account: Label(account.into()), window, used: Percent(used), resets_at: Some(Unix(now + resets_in_s)), read_at: Unix(now - 60), source: Source::Header }
}

fn with_accounts(host: &Host, labels: &[&str]) {
    let profiles = host.home.path().join(".local/state/north/profiles");
    let mut policy = String::new();
    for label in labels {
        let dir = profiles.join(format!("claude-{label}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
        std::fs::write(dir.join(".claude.json"), format!(r#"{{"oauthAccount":{{"accountUuid":"login-{label}"}}}}"#)).unwrap();
        policy.push_str(&format!("[accounts.claude.profiles.{label}]\n\"{}\" = {{ path = \"{}\" }}\n", crate::platform::host::name(), dir.display()));
    }
    let file = crate::pace::target::home_policy_path(host.home.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, policy).unwrap();
}

fn working_on(host: &Host, name: &str, account: &str, parent: Option<&str>, lane: &Path) {
    let _frames = live_with(host, name, "Not done: working.", Some("north#259"));
    let file = host.root().join(name).join("session.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    record["account"] = json!(account);
    record["cwd"] = json!(lane);
    record["parent"] = json!(parent);
    std::fs::write(file, record.to_string()).unwrap();
}

fn lane(host: &Host, name: &str) -> PathBuf {
    let lane = host.home.path().join("lanes").join(name);
    std::fs::create_dir_all(&lane).unwrap();
    std::fs::write(lane.join("work.rs"), "unlanded").unwrap();
    lane
}

type Launches = std::cell::RefCell<Vec<crate::fleet::model::LaunchRequest>>;

fn hand_over_with(host: &Host, table: &Table, launches: &Launches) -> crate::fleet::handover::HandOver {
    hand_over_telling(host, table, launches, &|_| Ok(()))
}

fn hand_over_telling(host: &Host, table: &Table, launches: &Launches, tell_tom: &dyn Fn(&str) -> Result<(), String>) -> crate::fleet::handover::HandOver {
    let take = |request: &crate::fleet::model::LaunchRequest| {
        launches.borrow_mut().push(request.clone());
        host.fleet.session(&request.name)
    };
    crate::fleet::handover::hand_over(&host.fleet, table, &|_| true, &take, tell_tom, NOW_MS)
}

fn parked_on(host: &Host, name: &str, account: &str, at_ms: u64, since_ms: u64, cwd: &Path) {
    use crate::fleet::waits::{Registered, Run};
    let wait = Registered { run: Run::Usage { account: account.into(), at_ms }, cwd: cwd.to_owned(), since_ms, label: None };
    std::fs::create_dir_all(host.root().join(".waits")).unwrap();
    std::fs::write(host.root().join(".waits").join(format!("{name}.json")), serde_json::to_vec(&[wait]).unwrap()).unwrap();
}

fn north_576() -> Table {
    use crate::usage::decide::world::{LimitEvent, Window};
    let (hour, day) = (3_600, 86_400);
    let stale_reset = -(122 * hour + 21 * 60);
    let mut stale = read(1, "a", Window::FiveHour, 30.0, stale_reset);
    stale.read_at.0 += stale_reset - 5 * hour + 60;
    let readings = vec![
        read(0, "a", Window::Week, 24.0, 4 * day),
        stale,
        read(2, "b", Window::Week, 100.0, 2 * day + 6 * hour),
        read(3, "c", Window::Week, 100.0, 6 * hour + 21 * 60),
        read(4, "d", Window::Week, 91.0, 3 * day),
        read(5, "d", Window::FiveHour, 3.0, 4 * hour),
    ];
    let limits = readings.iter().filter(|reading| reading.used.0 >= 99.5).map(|reading| LimitEvent { account: reading.account.clone(), window: reading.window, resets_at: reading.resets_at, at: reading.read_at }).collect();
    Table(crate::fleet::handover::Observed { readings, limits })
}

fn on_north_576(host: &Host) -> u64 {
    with_accounts(host, &["a", "b", "c", "d"]);
    let c_resets_ms = NOW_MS + (6 * 3_600 + 21 * 60) * 1000;
    working_on(host, "lead", "c", None, &lane(host, "lead"));
    for (name, account) in [("proxy", "a"), ("worker", "d")] {
        let cwd = lane(host, name);
        working_on(host, name, account, Some("lead"), &cwd);
        parked_on(host, name, account, c_resets_ms, NOW_MS - 30 * 60_000, &cwd);
    }
    c_resets_ms
}

#[test]
fn north_576_with_a_and_d_reading_headroom_after_their_parks_the_lead_on_c_goes_to_a_or_d_and_tom_hears_no_all_out() {
    use crate::usage::decide::world::{Account, Host as Machine, Label, Provider, Rate, Rates, Settings, Unix, Window, World};
    use crate::usage::decide::{View, view};
    let host = host("nexus");
    on_north_576(&host);
    let table = north_576();
    let launches = Launches::default();

    let done = hand_over_with(&host, &table, &launches);

    assert_eq!(done.alert, None, "a and d have headroom: {:?}", done.alert);
    let launched = launches.borrow();
    assert_eq!(launched.iter().map(|request| request.name.as_str()).collect::<Vec<_>>(), ["lead"]);
    assert!(["a", "d"].contains(&launched[0].account.as_str()), "{}", launched[0].account);
    let rate = Rate::Pooled { points_per_usd: 1.0, n: 0 };
    let account = Account { provider: Provider::Claude, label: Label("a".into()), weight: 1.0, signed_in: [Machine("nexus".into())].into(), extra_usage_enabled: Some(false), extra_usage_spend: None, rates: Rates { five_hour: rate, week: rate } };
    let settings = Settings::default();
    let world = World { now: Unix((NOW_MS / 1000) as i64), accounts: &[account], readings: &table.0.readings, limits: &table.0.limits, sessions: &[], tripped: &[], credit: None, settings: &settings };
    assert!(matches!(view(&world, &Label("a".into()), Window::FiveHour), View::Stale { .. }), "a's 5-hour reading whose reset passed is unknown, not out");
}


const HANG: Duration = Duration::from_secs(30);

struct Sleepy;

impl WaitProbes for Sleepy {
    fn placement(&self) -> Option<Box<dyn FnOnce() + Send>> {
        Some(Box::new(|| std::thread::sleep(HANG)))
    }

    fn jobs(&self) -> Vec<ProbeJob> {
        let said = |line: &str| Probed { lines: vec![line.to_owned()], settled: Vec::new() };
        vec![
            ProbeJob { name: "probes.quick", run: Box::new(move |_| said("quick ran")) },
            ProbeJob {
                name: "probes.slow",
                run: Box::new(move |_| {
                    std::thread::sleep(HANG);
                    said("slow ran")
                }),
            },
            ProbeJob { name: "probes.after", run: Box::new(move |_| said("after ran")) },
        ]
    }
}

struct HungLedger;

impl LedgerHost for HungLedger {
    fn exchange(&mut self, _: &Exchange) -> NorthResult<Exchanged> {
        std::thread::sleep(HANG);
        Err(NorthError::Refused("ssh nexus: hung".into()))
    }

    fn settle(&mut self, _: &[WaitKey]) -> NorthResult<()> {
        std::thread::sleep(HANG);
        Err(NorthError::Refused("ssh nexus: hung".into()))
    }
}

struct HungTake;

impl FarSpools for HungTake {
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        std::thread::sleep(HANG);
        Nobody.take(ssh)
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        Nobody.drop(ssh, ids)
    }

    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        Nobody.set_aside(ssh, ids)
    }

    fn cache(&self, ssh: &str, table: &FleetTable) -> NorthResult<()> {
        Nobody.cache(ssh, table)
    }

    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry> {
        Nobody.registry(ssh)
    }

    fn deliver(&self, ssh: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Nobody.deliver(ssh, rows)
    }
}

#[test]
fn a_tick_whose_ledger_exchange_placement_take_far_and_a_probe_hang_past_their_budgets_finishes_naming_each_deferred_stage() {
    let host = host("nexus");
    let pulls = Pulls { spools: Arc::new(HungTake), always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &open_ledger(|| Ok(HungLedger)), &pulls, &Sleepy, NOW_MS, Budgets::each(Duration::from_millis(300))).unwrap();

    assert!(ticked.wall < Duration::from_secs(5), "the hung stages held the tick for {:?}", ticked.wall);
    let lines = ticked.lines();
    for stage in ["take_far on gex45", "ledger.exchange", "placement"] {
        assert!(lines.contains(&format!("deferred {stage} to the next tick: still running after its 300ms budget")), "{stage}: {lines:?}");
    }
    assert!(lines.iter().any(|line| line == "quick ran"), "the probes still run after a deferred exchange: {lines:?}");
    assert!(lines.iter().any(|line| line == "deferred probes.slow to the next tick: still running when probes spent their 300ms budget"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "deferred probes.after to the next tick: probes spent their 300ms budget before it began"), "{lines:?}");
    let timing = ticked.timing();
    assert!(timing.contains(", probes.quick ") && timing.contains(", probes.slow "), "{timing}");
    assert!(Budgets::TICK.worst() < Duration::from_secs(40), "the tick's stage budgets sum to {:?}, past 40 s of its 50 s start timeout", Budgets::TICK.worst());
}
