use super::{Told, announce, scan};
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
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": "goals: 4 open, 2 held"}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{UNIT}: Failed with result 'exit-code'.")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": "Failed to start fleet-tick: flush this host's north fleet spool."}).to_string(),
    ]
}

fn journal(starts: usize, start: &dyn Fn(usize) -> Vec<String>) -> String {
    (0..starts).flat_map(start).collect::<Vec<_>>().join("\n")
}

fn tick(told: &mut Told, journal: &str) -> Vec<String> {
    let mut sent = Vec::new();
    announce(&scan(journal), told, &mut |message| {
        sent.push(message.to_owned());
        true
    });
    sent
}

#[test]
fn health_notice_fires_on_threshold_cause_change_doubling_and_recovery_only() {
    let goals = |n: usize| timed_out_start(&format!("goals: {n} open, {} held, claude:a week 9{n}%", n + 1));
    let mut told = Told::default();

    assert_eq!(tick(&mut told, &journal(2, &goals)), Vec::<String>::new(), "under threshold");

    let first = tick(&mut told, &journal(3, &goals));
    assert_eq!(first, vec![format!("harness unit {UNIT} failed to start 3 times in the last hour: start operation timed out")]);
    assert!(!first[0].contains("goals"), "names the unit's own cause, not its last log line");

    for starts in [3, 4, 5, 4] {
        assert_eq!(tick(&mut told, &journal(starts, &goals)), Vec::<String>::new(), "same cause at {starts} starts repeats nothing");
    }

    assert_eq!(tick(&mut told, &journal(6, &goals)), vec![format!("harness unit {UNIT} failed to start 6 times in the last hour: start operation timed out")]);
    assert_eq!(tick(&mut told, &journal(11, &goals)), Vec::<String>::new(), "under double of the last notice");

    let exited = |_| exited_start();
    assert_eq!(
        tick(&mut told, &journal(6, &exited)),
        vec![format!("harness unit {UNIT} failed to start 6 times in the last hour: ssh nexus: timed out; remote outcome unknown")]
    );
    assert_eq!(tick(&mut told, &journal(7, &exited)), Vec::<String>::new());

    assert_eq!(tick(&mut told, &journal(2, &exited)), vec![format!("harness unit {UNIT} recovered: under 3 failed starts in the last hour")]);
    assert_eq!(tick(&mut told, &journal(0, &exited)), Vec::<String>::new(), "recovery is told once");
    assert_eq!(told, Told::default());
}
