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
    "run-bounded 30m -- bun /tmp/task.mjs",
    "run-bounded 30m -- bun task > /tmp/task.log 2>&1 &",
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

#[test]
fn full_command_patterns_that_match_their_own_command_line_are_refused() {
    for command in [
        "pgrep -f 'safe-push --to main'",
        "pkill -f 'safe-push --to main'",
        "pgrep -fa 'safe-push --to main'",
        "pkill -9 -f 'cargo test'",
        "pkill -TERM -f 'cargo test'",
        "pgrep --full 'cargo test'",
        "pgrep -u tom -f 'cargo test'",
        "pgrep -f \"cargo test\" || echo none",
        "pgrep -f cargo; cargo test",
        "pgrep -fi 'CARGO TEST'",
        "pkill -f 'cargo.*test'",
        "sudo pkill -f 'cargo test'",
        "echo $(pgrep -f 'cargo test')",
        "bash -c \"pgrep -f 'cargo test'\"",
        "ssh gex45 pgrep -f 'cargo test'",
        "while pgrep -f 'cargo test'; do sleep 1; done",
    ] {
        let hit = decide(command).unwrap_or_else(|| panic!("allowed {command}"));
        assert!(matches!(hit, Hit::SelfMatch { .. }), "{command}: {hit:?}");
        let reason = reason(&hit);
        assert!(reason.contains("self-matching"), "{command}");
        assert!(reason.contains("bracket form"), "{command}");
        assert!(reason.contains("pid file"), "{command}");
    }
    let reason = reason(&decide("pgrep -f 'cargo test'").unwrap());
    assert!(reason.contains("'[c]argo test'"));
}

#[test]
fn bracket_form_pid_files_and_non_full_matches_pass() {
    for command in [
        "pgrep -f '[c]argo test'",
        "pkill -f '[s]afe-push --to main'",
        "pgrep -fa '[c]argo test'",
        "pkill -F /tmp/task.pid",
        "kill $(cat /tmp/task.pid)",
        "pgrep cargo",
        "pgrep -x cargo",
        "pgrep -fx 'cargo test'",
        "pgrep -fA 'cargo test'",
        "pgrep -f --ignore-ancestors 'cargo test'",
        "pgrep -u tom cargo",
        "echo 'pgrep -f cargo'",
        "cat <<'BODY'\npgrep -f cargo\nBODY",
        "pgrep -f \"$PATTERN\"",
    ] {
        assert!(!denied(command), "allowed {command}");
    }
}

#[test]
fn bracket_form_still_refused_when_its_target_text_is_elsewhere_in_the_command() {
    assert!(denied("pgrep -f '[c]argo test'; cargo test"));
    assert!(!denied("pgrep -f '[c]argo test'; cargo build"));
}

const TERMINAL_CGROUP: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-niri-ghostty-898870.scope\n";
const SERVICE_CGROUP: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/run-r4242.service\n";

fn unit_reason(command: &str, cgroup: &str) -> String {
    match decide_in(command, cgroup).unwrap_or_else(|| panic!("allowed {command}")) {
        hit @ Hit::Teardown { host: None, .. } => reason(&hit),
        hit => panic!("{command}: {hit:?}"),
    }
}

fn assert_safe_alternative(reason: &str, command: &str) {
    assert!(reason.contains("terminal"), "{command}");
    assert!(
        reason.contains("`systemd-run --user --unit=NAME ...`"),
        "{command}"
    );
    assert!(reason.contains("`systemctl --user stop NAME`"), "{command}");
    assert!(reason.contains("own cgroup path"), "{command}");
}

#[test]
fn terminal_scopes_and_app_units_are_refused_with_the_teardown_reason() {
    for verb in ["stop", "kill", "restart", "try-restart", "force-reload"] {
        for flags in ["", "--user "] {
            for unit in [
                "app-niri-ghostty-898870.scope",
                "app-niri-ghostty-898870.service",
                "app-*.scope",
                "'app-*.service'",
                "*.scope",
                "app-org.gnome.Nautilus@1234.service",
            ] {
                let command = format!("systemctl {flags}{verb} {unit}");
                let reason = unit_reason(&command, "");
                assert!(reason.contains("terminal-scope teardown"), "{command}");
                assert_safe_alternative(&reason, &command);
            }
        }
    }
    let command = "systemctl --user stop north-landing-train5 app-niri-ghostty-1.scope";
    assert!(unit_reason(command, "").contains("terminal-scope teardown"));
    assert!(denied("bash -c 'systemctl --user stop app-niri-ghostty-1.scope'"));
    assert!(denied("echo ok; sudo systemctl stop app-niri-ghostty-1.scope"));
}

#[test]
fn the_callers_own_unit_and_every_parent_on_its_cgroup_path_are_refused() {
    for unit in [
        "app-niri-ghostty-898870.scope",
        "app.slice",
    ] {
        let command = format!("systemctl --user stop {unit}");
        assert_safe_alternative(&unit_reason(&command, TERMINAL_CGROUP), &command);
    }
    for unit in ["run-r4242.service", "run-r4242", "app.slice"] {
        let command = format!("systemctl --user restart {unit}");
        let reason = unit_reason(&command, SERVICE_CGROUP);
        assert!(reason.contains("own-session unit teardown"), "{command}");
        assert_safe_alternative(&reason, &command);
    }
    assert!(decide_in("systemctl --user stop run-r4242", TERMINAL_CGROUP).is_none());
    assert!(decide_in("systemctl --user stop run-r9.service", SERVICE_CGROUP).is_none());
}

#[test]
fn operands_the_guard_cannot_see_are_refused_even_with_a_fixed_suffix() {
    for command in [
        "systemctl --user stop $unit",
        "systemctl --user stop \"$unit\"",
        "systemctl --user stop ${unit}",
        "systemctl --user stop \"${unit}\"",
        "systemctl stop $(cat /tmp/unit.name)",
        "systemctl --user stop \"$(cat /tmp/unit.name)\"",
        "systemctl --user stop `cat /tmp/unit.name`",
        "systemctl --user stop \"$unit\".service",
        "systemctl --user stop north-landing-$id",
        "systemctl --user stop \"${prefix}-train5.service\"",
        "systemctl --user restart \"$(basename $path)\"",
        "unit=$(sed -n 's/.*://p' /proc/$pid/cgroup | xargs basename); systemctl --user stop \"$unit\"",
        "pgrep -f '[g]hostty' | xargs systemctl --user stop",
        "ps -o unit= -p 1 | xargs -r systemctl stop",
        "cat /proc/1/cgroup | xargs -I{} systemctl --user kill {}",
        "pgrep -f '[g]hostty' | xargs -n1 sh -c 'systemctl --user stop \"$0\"'",
        "for pid in $(pgrep -f '[g]hostty'); do u=$(basename $(cat /proc/$pid/cgroup)); systemctl --user stop \"$u\"; done",
    ] {
        let reason = unit_reason(command, "");
        assert!(reason.contains("unseen unit operand teardown"), "{command}");
        assert!(reason.contains("fixed suffix"), "{command}");
        assert_safe_alternative(&reason, command);
    }
}

#[test]
fn literal_launched_units_and_read_only_verbs_stay_allowed() {
    for command in [
        "systemctl --user stop north-landing-train5",
        "systemctl --user stop north-landing-train5.service",
        "systemctl stop north-land.service",
        "systemctl --user stop 'firn-*.service'",
        "systemctl --user stop firn-build.service north-land.service",
        "systemctl --user restart wob.service",
        "systemd-run --user --unit=north-landing-train5 sleep 30",
        "ssh gex45 'systemctl --user stop farm-runner-wisp-1.service'",
        "ssh gex45 'systemctl --user stop app-niri-ghostty-1.scope'",
        "systemctl --user status app-niri-ghostty-898870.scope",
        "systemctl --user show -p MainPID app-niri-ghostty-898870.scope",
        "systemctl --user cat app-niri-ghostty-898870.scope",
        "systemctl --user list-units 'app-*.scope'",
        "systemctl --user status $unit",
        "systemctl --user show \"$(cat /tmp/unit.name)\"",
        "systemctl --user cat \"${unit}\"",
        "echo systemctl --user stop app-niri-ghostty-898870.scope",
        "echo 'systemctl --user stop \"$unit\"'",
        "cat /proc/self/cgroup | xargs echo",
        "xargs systemctl --user status < /tmp/units",
    ] {
        for cgroup in ["", TERMINAL_CGROUP, SERVICE_CGROUP] {
            assert!(decide_in(command, cgroup).is_none(), "{command}");
        }
    }
}

#[test]
fn cgroup_units_lists_every_unit_on_the_path() {
    assert_eq!(
        commands::cgroup_units(TERMINAL_CGROUP),
        [
            "user.slice",
            "user-1000.slice",
            "user@1000.service",
            "app.slice",
            "app-niri-ghostty-898870.scope"
        ]
    );
    assert!(commands::cgroup_units("0::/\n").is_empty());
    assert!(commands::cgroup_units("").is_empty());
}
