use super::*;

const TRIGGERS: &[&str] = &[
    "kill -1",
    "kill -9 -1",
    "kill -- -1",
    "pkill -u tom",
    "pkill niri",
    "killall -u tom",
    "killall niri",
    "loginctl terminate-user tom",
    "systemctl --user exit",
    "systemctl --user restart niri.service",
    "systemctl restart user@1000.service",
    "ssh gex45 'systemctl restart user@1000.service'",
    "nohup bun task",
    "setsid bun task",
    "disown",
    "bun /tmp/task.mjs",
];

const NEAR_MISSES: &[&str] = &[
    "kill -1 1234",
    "kill -9 1234",
    "pkill -u tom scoped",
    "systemctl --user restart wob.service",
    "north run-bounded 30m -- bun /tmp/task.mjs",
    "bun task > /tmp/task.log 2>&1",
    "kill -9 1234 2>/dev/null &>/dev/null",
    "bun /tmp/claude-1000/proj/session/scratchpad/check.ts",
];

const PREFIXES: &[&str] = &[
    "",
    "CHECK=1 ",
    "CHECK=\"some value\" ",
    "command ",
    "sudo ",
    "env CHECK=1 ",
    "CHECK=1 command sudo env OTHER=2 ",
    "CHECK_0=17 sudo env command ",
    "sudo -u tom ",
    "env -u CHECK CHECK=1 ",
    "CHECK=${HOME} ",
    "CHECK=$HOME ",
];

fn denied(command: &str) -> bool {
    decide(command).is_some()
}

fn single(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

fn double(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

#[test]
fn recorded_fixtures_hold() {
    let cases: Vec<(String, String)> =
        serde_json::from_str(include_str!("fixtures/session_kill.json")).unwrap();
    for (command, expected) in cases {
        let observed = if denied(&command) { "deny" } else { "allow" };
        assert_eq!(observed, expected, "{command}");
    }
}

#[test]
fn live_invocations_are_refused_through_prefixes_chaining_and_substitution() {
    for trigger in TRIGGERS {
        assert!(denied(trigger), "trigger {trigger}");
        let substitution = format!("CHECK=$({trigger}); echo $CHECK");
        assert!(denied(&substitution), "substitution {substitution}");
        for prefix in PREFIXES {
            let wrapped = format!("{prefix}{trigger}");
            assert!(denied(&wrapped), "prefix {wrapped}");
            for separator in [";", "&&", "|", "\n"] {
                let chained = format!("printf ready {separator} {wrapped}");
                assert!(denied(&chained), "chained {chained:?}");
            }
        }
    }
}

#[test]
fn quoted_and_heredoc_mentions_stay_inert() {
    for trigger in TRIGGERS {
        let (single, double) = (single(trigger), double(trigger));
        for data in [&single, &double] {
            assert_eq!(denied(data), *trigger == "disown", "bare quoted {data}");
        }
        for data in [
            format!("echo {single}"),
            format!("echo {double}"),
            format!("git commit -m {single}"),
            format!("cat <<'BODY'\n{trigger}\nBODY"),
            format!("echo {trigger}"),
        ] {
            assert!(!denied(&data), "inert {data}");
        }
        for separator in [";", "&&", "|", "\n", "("] {
            let prose = format!("example {separator} {trigger}");
            for data in [
                format!("echo {}", self::single(&prose)),
                format!("echo {}", self::double(&prose)),
            ] {
                assert!(!denied(&data), "quoted separators {data:?}");
            }
        }
    }
    for command in NEAR_MISSES {
        assert!(!denied(command), "near miss {command}");
    }
}

#[test]
fn remote_teardown_names_the_host_command() {
    let hit = decide(
        "ssh -o BatchMode=yes root@gex45 'hostname; systemctl restart user@1000.service; echo rc $?'",
    )
    .unwrap();
    assert_eq!(
        hit,
        Hit::Teardown {
            shape: "user-manager teardown",
            host: Some("gex45".into())
        }
    );
    assert!(reason(&hit).contains("`firn host restart-user-manager gex45`"));
    assert!(reason(&decide("loginctl terminate-user tom").unwrap()).contains(HOST_RESTART));
}
