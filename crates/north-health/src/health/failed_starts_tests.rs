use super::{LIMIT, Told, WINDOW_MS, announce, scan};
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

fn tick(told: &mut Told, journal: &str, now: u64) -> Vec<String> {
    let mut sent = Vec::new();
    announce(&scan(journal), told, now, &mut |message| {
        sent.push(message.to_owned());
        true
    });
    sent
}

fn refreshed_start(sha: &str, canary: &str) -> Vec<String> {
    vec![
        json!({"_SYSTEMD_USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "north", "MESSAGE": format!("runtime follower failed to install main {sha} for {canary}")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("{UNIT}: Failed with result 'exit-code'.")}).to_string(),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": "Failed to start fleet-tick: flush this host's north fleet spool."}).to_string(),
    ]
}

#[test]
fn a_repeated_failed_start_with_a_fresh_runtime_sha_or_canary_pages_once() {
    let mut told = Told::default();
    let start = WINDOW_MS * 2;
    let first = tick(&mut told, &journal(3, &|_| refreshed_start("636c151b", "canary-opencode-4101624")), start);
    assert_eq!(first, vec![format!("harness unit {UNIT} failed to start 3 times in the last hour: runtime follower failed to install main 636c151b for canary-opencode-4101624")]);

    for starts in [4, 5, 13] {
        assert_eq!(
            tick(&mut told, &journal(starts, &|_| refreshed_start("22272a0b", "canary-opencode-4123229")), start + WINDOW_MS - 1),
            Vec::<String>::new(),
            "a moved count, runtime sha and canary session id are one cause at {starts} starts"
        );
    }
}

#[test]
fn a_repeated_failed_start_with_the_same_cause_pages_once() {
    let goals = |n: usize| timed_out_start(&format!("goals: {n} open, {} held, claude:a week 9{n}%", n + 1));
    let mut told = Told::default();
    let start = WINDOW_MS * 2;

    assert_eq!(tick(&mut told, &journal(2, &goals), start), Vec::<String>::new(), "under threshold");

    let first = tick(&mut told, &journal(3, &goals), start);
    assert_eq!(first, vec![format!("harness unit {UNIT} failed to start 3 times in the last hour: start operation timed out")]);
    assert!(!first[0].contains("goals"), "names the unit's own cause, not its last log line");

    for starts in [3, 4, 5, 6, 11] {
        assert_eq!(tick(&mut told, &journal(starts, &goals), start + WINDOW_MS - 1), Vec::<String>::new(), "same cause at {starts} starts repeats nothing within six hours");
    }

    assert_eq!(
        tick(&mut told, &journal(6, &goals), start + WINDOW_MS),
        vec![format!("harness unit {UNIT} failed to start 6 times in the last hour: start operation timed out")],
        "a repeat of the same key after six hours pages again"
    );

    let exited = |_| exited_start();
    assert_eq!(
        tick(&mut told, &journal(6, &exited), start + WINDOW_MS + 1),
        vec![format!("harness unit {UNIT} failed to start 6 times in the last hour: ssh nexus: timed out; remote outcome unknown")],
        "a changed cause pages at once and names the first error line"
    );
    assert_eq!(tick(&mut told, &journal(7, &exited), start + WINDOW_MS + 2), Vec::<String>::new());
    assert_eq!(
        tick(&mut told, &journal(7, &goals), start + WINDOW_MS + 2),
        Vec::<String>::new(),
        "the earlier cause keeps its own six hours"
    );

    assert_eq!(
        tick(&mut told, &journal(2, &exited), start + WINDOW_MS + 3),
        vec![format!("harness unit {UNIT} recovered: under 3 failed starts in the last hour")]
    );
    assert_eq!(tick(&mut told, &journal(0, &exited), start + WINDOW_MS + 4), Vec::<String>::new(), "recovery is told once");
    assert_eq!(told, Told::default());

    let next = start + WINDOW_MS + 10;
    assert_eq!(tick(&mut told, &journal(3, &goals), next), vec![format!("harness unit {UNIT} failed to start 3 times in the last hour: start operation timed out")]);
    let started = format!(
        "{}\n{}",
        journal(3, &goals),
        json!({"USER_UNIT": UNIT, "SYSLOG_IDENTIFIER": "systemd", "MESSAGE": format!("Started {UNIT}.")})
    );
    assert!(scan(&started).is_empty(), "a successful start after the last failure clears the failed-start streak");
    assert_eq!(
        tick(&mut told, &started, next + 1),
        vec![format!("harness unit {UNIT} recovered: under {LIMIT} failed starts in the last hour")],
        "a unit that started since its last failure is sent as recovered, not as failing"
    );
}
