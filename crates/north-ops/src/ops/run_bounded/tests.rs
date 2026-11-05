use std::collections::BTreeSet;

use super::{Request, forwarded, guard_arguments, parse, seconds, serve_arguments};

fn words(text: &[&str]) -> Vec<String> {
    text.iter().map(|word| (*word).to_owned()).collect()
}

fn request() -> Request {
    parse(&words(&["30m", "--", "bun", "task.mjs"])).unwrap()
}

#[test]
fn a_duration_outside_one_second_to_twenty_four_hours_or_a_missing_separator_or_command_is_a_usage_error() {
    for arguments in [&["0s", "--", "true"][..], &["25h", "--", "true"], &["30m", "true"], &["30m", "--"], &["30m"], &["1ms", "--", "true"], &["100000", "--", "true"], &["h", "--", "true"], &["-5", "--", "true"]] {
        assert_eq!(parse(&words(arguments)), None, "{arguments:?}");
    }
    assert_eq!(seconds("24h"), Some(86_400));
    assert_eq!(seconds("1d"), Some(86_400));
    assert_eq!(seconds("1441m"), None);
    assert_eq!(seconds("00090"), Some(90));
    assert_eq!(seconds("45"), Some(45));
    assert_eq!(seconds("10s"), Some(10));
}

#[test]
fn the_guard_stage_enters_north_in_a_scope_bound_to_the_fleet_scope_without_a_user_namespace() {
    let staged = guard_arguments(&request(), "agent-bounded-1-2-3", Some("north-session-x.scope"), "/n/north");
    assert_eq!(
        staged,
        words(&[
            "--user", "--scope", "--quiet", "--collect", "--same-dir",
            "--property=BindsTo=north-session-x.scope", "--property=After=north-session-x.scope",
            "--unit=agent-bounded-1-2-3-guard", "--slice=agent-bounded.slice",
            "/n/north", "run-bounded", "30m", "--", "bun", "task.mjs",
        ])
    );
    assert!(!staged.iter().any(|word| word.contains("user-namespace") || word == "-p" || word.starts_with("--property=PrivateUsers")));
    assert!(!guard_arguments(&request(), "u", None, "/n/north").iter().any(|word| word.starts_with("--property=")));
}

#[test]
fn the_serve_stage_binds_the_service_to_its_guard_limits_its_runtime_and_forwards_the_environment_except_systemd_own_names() {
    let names: BTreeSet<String> = ["HOME", "NOTIFY_SOCKET", "LISTEN_FDS", "1BAD", "A-B", "_OK", "INVOCATION_ID"].map(str::to_owned).into();
    let served = serve_arguments(&request(), "agent-bounded-1-2-3", &names);
    assert_eq!(
        served,
        words(&[
            "--user", "--pipe", "--quiet", "--wait", "--collect", "--same-dir",
            "-E", "HOME", "-E", "_OK",
            "--unit=agent-bounded-1-2-3", "--slice=agent-bounded.slice",
            "--property=BindsTo=agent-bounded-1-2-3-guard.scope", "--property=After=agent-bounded-1-2-3-guard.scope",
            "--property=RuntimeMaxSec=1800s",
            "timeout", "--preserve-status", "--signal=TERM", "--kill-after=5s", "30m", "bun", "task.mjs",
        ])
    );
    assert!(forwarded("PATH") && !forwarded("JOURNAL_STREAM") && !forwarded(""));
}
