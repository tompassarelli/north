use super::{Loop, Reading, Reader, Hosts, Role, check, last_written, local_ages, parse, REGISTRY, plumbing};
use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, SystemTime};

const FIXTURE: &str = r#"
[[loop]]
name = "tier"
decision = "tier per category"
sites = ["a.rs"]
signal = "cost per kept box"
updated_by = "north tune"
cadence_hours = 24
record = "north/tune/cells.json"
host = "ledger"

[[loop]]
name = "deadline"
decision = "land check deadline"
sites = ["b.rs"]
signal = "p90 pass wall"
updated_by = "north land"
cadence_hours = 24
record = "safe-push/check-timings/passes"
host = "lander"
"#;

fn written(dir: &Path, record: &str, at: SystemTime) {
    let path = dir.join(record);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(&path).unwrap().set_modified(at).unwrap();
}

fn hours(count: u64) -> Duration {
    Duration::from_secs(count * 3_600)
}

fn epoch(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

fn nexus(lander: Result<&str, &str>) -> Hosts {
    Hosts { here: "nexus".into(), ledger: Some("nexus".into()), lander: lander.map(str::to_owned).map_err(str::to_owned) }
}

fn checked(hosts: &Hosts, dir: &Path, now: SystemTime, ssh: &dyn Fn(&str) -> Result<String, String>) -> super::Checked {
    let reader = Reader::new(hosts, dir, ssh);
    check(&parse(FIXTURE).unwrap(), &|found, record| reader.read(found, record), now)
}

fn no_ssh(host: &str) -> Result<String, String> {
    panic!("read {host} over ssh")
}

#[test]
fn a_loop_past_twice_its_cadence_fails_and_one_inside_passes() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now - hours(3));
    written(dir.path(), "safe-push/check-timings/passes", now - hours(49));
    let found = checked(&nexus(Ok("nexus")), dir.path(), now, &no_ssh);
    assert_eq!(found.failing, vec!["deadline".to_owned()]);
    assert!(found.lines[0].starts_with("tier: ok: north tune updated 3 h ago"), "{:?}", found.lines);
    assert!(found.lines[1].starts_with("deadline: FAIL stale: north land last updated 49 h ago"), "{:?}", found.lines);
    assert_eq!(found.lines[2], "loops: 1 of 2 failing");
}

#[test]
fn a_closed_loop_whose_record_is_missing_fails_never_updated() {
    let dir = tempfile::tempdir().unwrap();
    let found = checked(&nexus(Ok("nexus")), dir.path(), SystemTime::now(), &no_ssh);
    assert_eq!(found.failing, vec!["tier".to_owned(), "deadline".to_owned()]);
    assert_eq!(found.lines[0], "tier: FAIL never updated: no north/tune/cells.json");
}

#[test]
fn an_open_loop_with_an_owner_prints_without_failing_and_one_without_fails() {
    let open = |owner: Option<&str>| Loop { name: "effort".into(), decision: "effort".into(), sites: vec!["a.rs".into()], constants: vec![], defaults: vec![], signal: None, updated_by: "by hand".into(), cadence_hours: 24, record: None, host: Role::Here, owner: owner.map(str::to_owned) };
    let read = |_: &Loop, _: &str| Reading::Never;
    let owned = check(&[open(Some("north#112 Effort is an arm"))], &read, SystemTime::now());
    assert_eq!(owned.failing, Vec::<String>::new());
    assert_eq!(owned.lines[0], "effort: open; by hand sets it; owner north#112 Effort is an arm");
    let unowned = check(&[open(None)], &read, SystemTime::now());
    assert_eq!(unowned.failing, vec!["effort".to_owned()]);
    assert_eq!(unowned.lines[0], "effort: FAIL no outcome signal and no owner; by hand sets it");
}

#[test]
fn a_loop_without_a_signal_needs_an_owner() {
    let unowned = FIXTURE.replacen("signal = \"cost per kept box\"\n", "", 1).replacen("record = \"north/tune/cells.json\"\n", "", 1);
    assert_eq!(parse(&unowned), Err("loop tier has no signal and no owner to close it".to_owned()));
}

#[test]
fn a_host_that_is_not_a_role_is_refused() {
    assert_eq!(parse(&FIXTURE.replacen("host = \"ledger\"", "host = \"nexus\"", 1)), Err("loop tier has host nexus, not ledger, lander or here".to_owned()));
}

#[test]
fn a_closed_loop_on_the_lander_host_reads_its_age_over_ssh() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now - hours(2));
    let asked = Cell::new(0);
    let ssh = |host: &str| {
        asked.set(asked.get() + 1);
        assert_eq!(host, "whiterabbit");
        Ok(format!(r#"{{"deadline": {}, "tier": null}}"#, epoch(now - hours(5))))
    };
    let found = checked(&nexus(Ok("whiterabbit")), dir.path(), now, &ssh);
    assert_eq!(found.failing, Vec::<String>::new());
    assert_eq!(found.lines[1], "deadline: ok: north land updated 5 h ago from p90 pass wall");
    assert_eq!(asked.get(), 1);
}

#[test]
fn a_remote_age_past_twice_the_cadence_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now);
    let found = checked(&nexus(Ok("whiterabbit")), dir.path(), now, &|_| Ok(format!(r#"{{"deadline": {}}}"#, epoch(now - hours(72)))));
    assert!(found.lines[1].starts_with("deadline: FAIL stale: north land last updated 72 h ago"), "{:?}", found.lines);
}

#[test]
fn an_unread_remote_source_reports_unread_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now);
    let found = checked(&nexus(Ok("whiterabbit")), dir.path(), now, &|_| Err("ssh exit 255".to_owned()));
    assert_eq!(found.failing, vec!["deadline".to_owned()]);
    assert_eq!(found.lines[1], "deadline: FAIL UNREAD: lander whiterabbit: ssh exit 255");
}

#[test]
fn a_reply_that_is_not_json_or_omits_the_loop_is_unread() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now);
    let garbled = checked(&nexus(Ok("whiterabbit")), dir.path(), now, &|_| Ok("welcome".to_owned()));
    assert!(garbled.lines[1].starts_with("deadline: FAIL UNREAD: lander whiterabbit: unreadable reply"), "{:?}", garbled.lines);
    let silent = checked(&nexus(Ok("whiterabbit")), dir.path(), now, &|_| Ok("{}".to_owned()));
    assert_eq!(silent.lines[1], "deadline: FAIL UNREAD: lander whiterabbit: reported no age for deadline");
}

#[test]
fn a_lander_host_nobody_names_is_unread() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now);
    let found = checked(&nexus(Err("no landing.lander-host")), dir.path(), now, &no_ssh);
    assert_eq!(found.lines[1], "deadline: FAIL UNREAD: lander host unresolved: no landing.lander-host");
}

#[test]
fn a_role_that_resolves_to_this_host_is_read_locally() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now - hours(1));
    written(dir.path(), "safe-push/check-timings/passes", now - hours(1));
    let found = checked(&nexus(Ok("nexus")), dir.path(), now, &no_ssh);
    assert_eq!(found.failing, Vec::<String>::new());
}

#[test]
fn ledger_without_a_remote_file_is_this_host() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now);
    let hosts = Hosts { here: "laptop".into(), ledger: None, lander: Ok("laptop".into()) };
    written(dir.path(), "safe-push/check-timings/passes", now);
    assert_eq!(checked(&hosts, dir.path(), now, &no_ssh).failing, Vec::<String>::new());
}

#[test]
fn the_lander_host_comes_from_git_config_of_the_safe_push_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".safe-push"), "[landing]\n\tlander-host = whiterabbit\n").unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".config/threads")).unwrap();
    std::fs::write(home.path().join(".config/threads/remote"), "tom@nexus\n").unwrap();
    let hosts = Hosts::read(home.path(), Ok(dir.path().to_path_buf()));
    assert_eq!((hosts.lander, hosts.ledger), (Ok("whiterabbit".to_owned()), Some("nexus".to_owned())));
    let unnamed = Hosts::read(home.path(), Ok(home.path().to_path_buf()));
    assert!(unnamed.lander.is_err());
}

#[test]
fn ages_lists_every_loop_with_a_record_as_epoch_seconds_or_null() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now - hours(3));
    let ages = local_ages(&parse(FIXTURE).unwrap(), dir.path());
    assert_eq!(ages["tier"], Some(epoch(last_written(dir.path(), "north/tune/cells.json").unwrap())));
    assert_eq!(ages["deadline"], None);
    assert_eq!(serde_json::to_string(&ages).unwrap().replace(char::is_numeric, "0"), r#"{"deadline":null,"tier":0000000000}"#);
}

#[test]
fn the_live_registry_parses_and_every_closed_loop_names_where_it_is_read() {
    let loops = parse(REGISTRY).unwrap();
    assert!(plumbing(REGISTRY).is_ok());
    assert!(loops.iter().any(|found| found.host == Role::Lander && found.record.is_some()));
    assert!(loops.iter().filter(|found| found.record.is_none()).all(|found| found.owner.is_some()));
}
