use super::{LIMIT, Told, announce, scan};
use serde_json::json;

const UNIT: &str = "fleet-tick.service";

fn timed_out_start(goals: &str) -> Vec<String> {
    vec![
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": goals}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{UNIT}: start operation timed out. Terminating.")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{UNIT}: Failed with result 'timeout'.")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": "Failed to start fleet-tick: flush this host's north fleet spool."}).to_string(),
    ]
}

fn exited_start() -> Vec<String> {
    vec![
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": "ssh nexus: timed out; remote outcome unknown"}).to_string(),
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": "fetch: cannot reach origin"}).to_string(),
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": "goals: 4 open, 2 held"}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{UNIT}: Failed with result 'exit-code'.")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": "Failed to start fleet-tick: flush this host's north fleet spool."}).to_string(),
    ]
}

fn journal(starts: usize, start: &dyn Fn(usize) -> Vec<String>) -> String {
    (0..starts).flat_map(start).collect::<Vec<_>>().join("\n")
}

fn tick(told: &mut Told, journal: &str) -> Vec<(String, String, bool)> {
    let mut sent = Vec::new();
    announce(&scan(journal), told, &mut |message, key, cleared| {
        sent.push((message.to_owned(), key.to_owned(), cleared));
        true
    });
    sent
}

const KEY: &str = "harness-unit-start:fleet-tick.service";

#[test]
fn a_failing_unit_names_its_own_cause_under_one_condition_on_every_check_and_its_recovery_clears_it() {
    let goals = |n: usize| timed_out_start(&format!("goals: {n} open, {} held, claude:a week 9{n}%", n + 1));
    let mut told = Told::default();

    assert_eq!(tick(&mut told, &journal(2, &goals)), Vec::new(), "under threshold");
    let failing = |starts: usize, cause: &str| vec![(format!("harness unit {UNIT} failed to start {starts} times in the last hour: {cause}"), KEY.to_owned(), false)];
    assert_eq!(tick(&mut told, &journal(3, &goals)), failing(3, "start operation timed out"), "names the unit's own cause, not its last log line");
    assert_eq!(tick(&mut told, &journal(4, &goals)), failing(4, "start operation timed out"), "every check repeats the condition and the fleet store delivers it once per episode");
    let exited = |_| exited_start();
    assert_eq!(tick(&mut told, &journal(6, &exited)), failing(6, "ssh nexus: timed out; remote outcome unknown"), "a changed cause names the first error line");

    let recovered = vec![(format!("harness unit {UNIT} recovered: under {LIMIT} failed starts in the last hour"), KEY.to_owned(), true)];
    assert_eq!(tick(&mut told, &journal(2, &exited)), recovered);
    assert_eq!(tick(&mut told, &journal(0, &exited)), Vec::new(), "recovery is told once");
    assert_eq!(told, Told::default());

    assert_eq!(tick(&mut told, &journal(3, &goals)), failing(3, "start operation timed out"));
    let started = format!("{}\n{}", journal(3, &goals), json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("Started {UNIT}.")}));
    assert!(scan(&started).is_empty(), "a successful start after the last failure clears the failed-start streak");
    assert_eq!(tick(&mut told, &started), recovered, "a unit that started since its last failure is sent as recovered, not as failing");
}

#[test]
fn a_failed_harness_check_started_by_a_firn_commit_pages_the_catalog_scan_that_failed_it_not_that_commit() {
    let unit = "firn-agent-config-check-fa9c3c1c0123456789abcdef0123456789abcdef.service";
    let start = |_| {
        vec![
            json!({"_SYSTEMD_USER_UNIT": unit, "SYSLOG_IDENTIFIER": "north", "MESSAGE": "  FAIL: policy-contract: read-reach: north_hook needs a catalog registration owned by a Rust module"}).to_string(),
            json!({"USER_UNIT": unit, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{unit}: Failed with result 'exit-code'.")}).to_string(),
            json!({"USER_UNIT": unit, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": "Failed to start Firn agent configuration advisory check."}).to_string(),
        ]
    };
    let sent = tick(&mut Told::default(), &journal(3, &start));
    let [(page, _, false)] = sent.as_slice() else { panic!("one failing page: {sent:?}") };
    assert!(page.contains("firn-agent-config-check") && page.contains("catalog scan") && page.contains("runtime tree"), "{page}");
    assert!(!page.contains("fa9c3c1c"), "the firn commit that started the check is no cause: {page}");
}
