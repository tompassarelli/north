use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::Duration;

use super::live::human_turns;
use super::*;

const FIXTURE: &str = include_str!("presence-nightly.fixture.json");

fn at(text: &str) -> Moment {
    parse(text).expect("fixture time")
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

fn travel() -> Away {
    let travel = &fixture()["travel"];
    Away {
        from: at(travel["from"].as_str().unwrap()),
        to: at(travel["to"].as_str().unwrap()),
        kind: travel["kind"].as_str().unwrap().to_owned(),
        said: travel["said"].as_str().unwrap().to_owned(),
    }
}

fn nightly() -> Signals {
    let fixture = fixture();
    let mut turns = Vec::new();
    for day in fixture["days"].as_array().unwrap() {
        let day = day.as_str().unwrap();
        let mut moment = at(&format!("{day}T{}:00Z", fixture["awake_from"].as_str().unwrap()));
        let end = at(&format!("{day}T{}:00Z", fixture["awake_until"].as_str().unwrap()));
        while moment <= end {
            turns.push(Turn { at: moment, label: Label::Exact });
            moment += Duration::minutes(15);
        }
    }
    Signals { turns, timezone: Zone::named(fixture["timezone"].as_str().unwrap()), ..Signals::default() }
}

fn turn(moment: Moment) -> Turn {
    Turn { at: moment, label: Label::Exact }
}

fn live(moment: Moment, active: bool) -> Option<Machine> {
    Some(Machine { at: Some(moment), active: Some(active), ..Machine::default() })
}

fn state(now: Moment, signals: &Signals) -> &'static str {
    presence(now, signals).state
}

#[test]
fn nightly_gap_forecasts_sleep_and_live_activity_overrides_it() {
    let signals = nightly();
    let result = presence(at("2026-10-14T17:30:00Z"), &signals);
    assert_eq!(result.state, "present");
    assert_eq!(result.next.state, Some("asleep"));
    assert_eq!(result.next.at.as_deref(), Some("2026-10-14T18:00:00Z"));
    let now = at("2026-10-14T19:00:00Z");
    assert_eq!(state(now, &signals), "asleep");
    let mut awake = Signals { desktop: live(now, true), ..signals.clone() };
    assert_eq!(state(now, &awake), "present");
    awake.desktop = live(now, false);
    assert_eq!(state(now, &awake), "asleep");
    assert_eq!(state(at("2026-10-15T01:00:00Z"), &signals), "unknown");
}

#[test]
fn live_activity_overrides_stated_day_until_input_expires() {
    let now = at("2026-10-15T05:00:00Z");
    let signals = Signals { away: vec![travel()], desktop: live(now, true), ..nightly() };
    let result = presence(now, &signals);
    assert_eq!(result.state, "present");
    assert_eq!(result.since, "2026-10-15T05:00:00Z");
    assert_eq!(result.next.at.as_deref(), Some("2026-10-15T05:15:00Z"));
    assert_ne!(state(at("2026-10-15T16:00:00Z"), &signals), "travelling");
}

#[test]
fn turn_presence_expires_and_games_never_establish_presence() {
    let now = at("2026-10-15T04:00:00Z");
    let mut signals = Signals { turns: vec![turn(now + Duration::minutes(1))], ..Signals::default() };
    assert_eq!(state(now, &signals), "unknown");
    signals.turns.push(turn(now - Duration::minutes(15)));
    assert_eq!(state(now, &signals), "present");
    assert_eq!(state(now + Duration::minutes(17), &signals), "unknown");
    assert_eq!(state(now, &Signals::default()), "unknown");
}

#[test]
fn typed_input_overrides_travel_only_for_the_presence_slot() {
    let now = at("2026-10-15T05:00:00Z");
    let signals = Signals { away: vec![travel()], turns: vec![turn(now)], ..Signals::default() };
    assert_eq!(state(now, &signals), "present");
    assert_eq!(state(now + Duration::minutes(*SLOT_MIN), &signals), "present");
    assert_eq!(state(now + Duration::minutes(*SLOT_MIN) + Duration::seconds(1), &signals), "travelling");
}

#[test]
fn game_left_open_cannot_override_sleep_or_supply_evidence() {
    let result = presence(at("2026-10-14T19:00:00Z"), &nightly());
    assert_eq!(result.state, "asleep");
    assert_eq!(result.evidence, vec![Evidence::Tag("learned-sleep")]);
}

#[test]
fn background_pool_waits_for_continuous_idle_and_input_resets_it() {
    let start = at("2026-10-15T04:00:00Z");
    let edge = start + Duration::seconds(*IDLE_REOPEN_SEC);
    let pool = |now: Moment, signals: &Signals| presence(now, signals).pool_state;
    let mut signals = Signals {
        idle: vec![IdleRow { at: start, idle: Some(true) }],
        desktop: Some(Machine { session_start: Some(start), ..live(edge, false).unwrap() }),
        laptop: Some(Machine { idle_since: Some(start), ..live(edge, false).unwrap() }),
        ..Signals::default()
    };
    let seen = |signals: &mut Signals, moment: Moment| {
        signals.desktop.as_mut().unwrap().at = Some(moment);
        signals.laptop.as_mut().unwrap().at = Some(moment);
    };
    assert_eq!(pool(edge - Duration::seconds(1), &signals), "unknown");
    assert_eq!(pool(edge, &signals), "away");
    signals.turns = vec![turn(edge - Duration::minutes(1))];
    assert_eq!(pool(edge, &signals), "present");
    assert_eq!(pool(edge + Duration::minutes(16), &signals), "unknown");
    seen(&mut signals, edge + Duration::minutes(29));
    assert_eq!(pool(edge + Duration::minutes(29), &signals), "away");
    for (minutes, idle) in [(29, false), (30, true), (31, true)] {
        signals.idle.push(IdleRow { at: edge + Duration::minutes(minutes), idle: Some(idle) });
    }
    seen(&mut signals, edge + Duration::minutes(59));
    assert_eq!(pool(edge + Duration::minutes(59), &signals), "unknown");
    assert_eq!(pool(edge + Duration::minutes(60), &signals), "away");
    assert_eq!(pool(edge, &Signals::default()), "unknown");
}

#[test]
fn idle_history_improves_the_forecast_without_claiming_live_presence() {
    let now = at("2026-10-15T04:00:00Z");
    let idle = vec![
        IdleRow { at: now - Duration::days(1) - Duration::hours(2), idle: Some(false) },
        IdleRow { at: now - Duration::days(1) + Duration::hours(2), idle: Some(true) },
    ];
    let zone = Zone::Fixed(0);
    let rows: Vec<&IdleRow> = idle.iter().collect();
    assert!(Forecast::new(now, &[], &rows, &zone).probability(now) > 0.5);
    assert_eq!(state(now, &Signals { idle, ..Signals::default() }), "unknown");
}

#[test]
fn named_zones_follow_their_daylight_rule_and_resolve_wall_time_to_the_earlier_offset() {
    let Some(york) = Zone::named("America/New_York") else { return };
    assert_eq!(york.offset_at(at("2040-07-01T12:00:00Z").unix_timestamp()), -4 * 3600);
    assert_eq!(york.offset_at(at("2040-01-01T12:00:00Z").unix_timestamp()), -5 * 3600);
    let wall = at("2026-11-01T01:30:00Z");
    let wall = time::PrimitiveDateTime::new(wall.date(), wall.time());
    assert_eq!(iso(york.resolve(wall)), "2026-11-01T05:30:00Z");
}

struct Rig {
    _dir: TempDir,
    root: PathBuf,
    proc: PathBuf,
}

fn stat(path: &Path, state: &str, started: &str) {
    let mut fields = vec![state.to_owned(), "1".to_owned()];
    fields.extend(std::iter::repeat_n("0".to_owned(), 17));
    fields.push(started.to_owned());
    fs::write(path, format!("123 (swayidle) {}", fields.join(" "))).unwrap();
}

type Environment = std::collections::BTreeMap<&'static str, String>;

fn environment(root: &Path, runtime: &Path, operator: &Path, convo: &Path, tz: Option<&str>) -> Environment {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut env = Environment::from([
        ("HOME", text(&root.join("home"))),
        ("XDG_STATE_HOME", text(root)),
        ("XDG_RUNTIME_DIR", text(runtime)),
        ("AGENTS_OPERATOR_DIR", text(operator)),
        ("CONVO_STATE", text(convo)),
        ("LAPTOP_STATE", text(&root.join("laptop.json"))),
    ]);
    if let Some(tz) = tz {
        env.insert("TZ", tz.to_owned());
    }
    env
}

fn read_live(env: &Environment, now: Moment, proc: &Path) -> Signals {
    live_signals(&|name| env.get(name).cloned(), now, proc)
}

#[test]
fn live_reader_and_cli_use_only_fixture_state_and_validate_the_seat_process() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (runtime, operator, convo) = (root.join("runtime"), root.join("operator"), root.join("convo"));
    for path in [&runtime, &operator, &convo] {
        fs::create_dir(path).unwrap();
    }
    let moment = "2026-10-15T05:00:00Z";
    let transcript = root.join("session.jsonl");
    fs::write(&transcript, format!("{}\n{}\n", json!({"origin": {"kind": "human"}}), json!({"origin": {"kind": "agent"}}))).unwrap();
    let db = Connection::open(convo.join("index-v4.db")).unwrap();
    db.execute_batch(
        "CREATE TABLE files(id INTEGER,path TEXT,provider TEXT,agent TEXT,cwd TEXT,off INTEGER);\
         CREATE TABLE msg(file_id INTEGER,line INTEGER,ts TEXT,role TEXT);CREATE INDEX msg_file ON msg(file_id,line);",
    )
    .unwrap();
    let home = root.join("home").to_string_lossy().into_owned();
    let size = fs::metadata(&transcript).unwrap().len() as i64;
    db.execute("INSERT INTO files VALUES(1,?,'anthropic',NULL,?,?)", (transcript.to_string_lossy(), &home, size)).unwrap();
    for line in [1, 2] {
        db.execute("INSERT INTO msg VALUES(1,?,?,'user')", (line, moment)).unwrap();
    }
    drop(db);
    let fresh = "2026-10-15T05:01:00Z";
    let mut text = fs::read_to_string(&transcript).unwrap();
    text.push_str(&format!("{}\n", json!({"origin": {"kind": "human"}, "timestamp": fresh})));
    fs::write(&transcript, text).unwrap();
    fs::write(operator.join("presence.toml"), "[[away]]\nfrom = \"2026-10-14T16:00:00Z\"\nto = \"2026-10-15T16:00:00Z\"\nkind = \"travel\"\nsaid = \"away\"\n").unwrap();
    fs::write(runtime.join("desktop-idle.json"), json!({"pid": 123, "started": "100", "idle": false}).to_string()).unwrap();
    let proc = root.join("proc");
    fs::create_dir_all(proc.join("123")).unwrap();
    stat(&proc.join("123/stat"), "S", "100");
    fs::write(root.join("laptop.json"), json!({"reason": "gaming", "by": "laptop-auto"}).to_string()).unwrap();
    fs::write(root.join("laptop-observations.json"), json!({"at": fresh}).to_string()).unwrap();
    let env = environment(root, &runtime, &operator, &convo, None);
    let signals = read_live(&env, at(fresh), &proc);
    assert_eq!(signals.desktop.as_ref().and_then(|desktop| desktop.active), Some(true));
    assert_eq!(signals.laptop, None);
    assert_eq!(signals.turns, vec![turn(at(moment)), turn(at(fresh))]);
    assert_eq!(state(at(moment), &read_live(&env, at(moment), &proc)), "present");
    stat(&proc.join("123/stat"), "S", "101");
    assert_eq!(read_live(&env, at(moment), &proc).desktop, None);
    assert_eq!(human_turns(at(fresh), &root.join("missing.db"), Path::new(&home)), Vec::new());
}

fn live_rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let now = at("2026-10-15T12:00:00Z");
    let proc = root.join("proc");
    fs::create_dir_all(proc.join("123")).unwrap();
    fs::write(proc.join("stat"), format!("btime {}\n", now.unix_timestamp() - 7200)).unwrap();
    stat(&proc.join("123/stat"), "S", "100");
    fs::write(root.join("desktop-idle.json"), json!({"pid": 123, "started": "100", "idle": true}).to_string()).unwrap();
    fs::create_dir_all(root.join("agents/presence")).unwrap();
    fs::write(root.join("agents/presence/idle.jsonl"), format!("{}\n", json!({"at": "2026-10-15T11:00:00Z", "idle": true}))).unwrap();
    observe(&root, json!({"at": iso(now), "idle": "2026-10-15T11:00:00Z"}));
    Rig { _dir: dir, root, proc }
}

fn observe(root: &Path, observation: Value) {
    fs::write(root.join("laptop-observations.json"), observation.to_string()).unwrap();
}

fn live_now(rig: &Rig, now: Moment) -> (&'static str, &'static str) {
    let env = environment(&rig.root, &rig.root, &rig.root.join("operator"), &rig.root.join("convo"), Some("UTC"));
    let result = presence(now, &read_live(&env, now, &rig.proc));
    (result.state, result.pool_state)
}

#[test]
fn dead_or_replaced_watcher_cannot_make_historical_idle_away() {
    let now = at("2026-10-15T12:00:00Z");
    for process in ["missing", "Z", "replaced", "new-session"] {
        let rig = live_rig();
        let path = rig.proc.join("123/stat");
        match process {
            "missing" => fs::remove_file(&path).unwrap(),
            "Z" => stat(&path, "Z", "100"),
            "replaced" => stat(&path, "S", "101"),
            _ => fs::write(rig.proc.join("stat"), format!("btime {}\n", now.unix_timestamp() - 1800)).unwrap(),
        }
        assert_eq!(live_now(&rig, now), ("unknown", "unknown"), "{process}");
    }
}

#[test]
fn active_laptop_overrides_idle_desktop() {
    let rig = live_rig();
    let now = at("2026-10-15T12:00:00Z");
    observe(&rig.root, json!({"at": iso(now), "active": iso(now)}));
    assert_eq!(live_now(&rig, now), ("present", "present"));
    let env = environment(&rig.root, &rig.root, &rig.root.join("operator"), &rig.root.join("convo"), Some("UTC"));
    let result = presence(now, &read_live(&env, now, &rig.proc));
    assert!(result.evidence.contains(&Evidence::Tag("laptop")));
}

#[test]
fn failed_laptop_observation_cannot_keep_cached_idle_away() {
    let rig = live_rig();
    for (seconds, expected) in [(120, "away"), (121, "unknown"), (600, "unknown")] {
        let now = at("2026-10-15T12:00:00Z") + Duration::seconds(seconds);
        assert_eq!(live_now(&rig, now), (expected, expected), "{seconds}");
    }
}

#[test]
fn stopped_watcher_cannot_make_historical_idle_away() {
    for process in ["T", "t"] {
        let rig = live_rig();
        stat(&rig.proc.join("123/stat"), process, "100");
        assert_eq!(live_now(&rig, at("2026-10-15T12:00:00Z")), ("unknown", "unknown"), "{process}");
    }
}

#[test]
fn missing_or_stale_machine_readings_cannot_make_idle_away() {
    let now = at("2026-10-15T12:00:00Z");
    assert_eq!(live_now(&live_rig(), now), ("away", "away"));
    let observations = [
        None,
        Some(json!({})),
        Some(json!({"at": "2026-10-15T11:00:00Z", "idle": "2026-10-15T11:00:00Z"})),
        Some(json!({"at": "2026-10-15T11:00:00Z", "active": "2026-10-15T11:00:00Z"})),
        Some(json!({"at": "2026-10-15T12:01:00Z", "idle": "2026-10-15T11:00:00Z"})),
    ];
    for observation in observations {
        let rig = live_rig();
        match &observation {
            None => fs::remove_file(rig.root.join("laptop-observations.json")).unwrap(),
            Some(value) => observe(&rig.root, value.clone()),
        }
        assert_eq!(live_now(&rig, now), ("unknown", "unknown"), "{observation:?}");
    }
}
