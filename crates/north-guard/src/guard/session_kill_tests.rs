use super::*;

fn denied(command: &str) -> bool {
    !decide(command).is_empty()
}

#[test]
fn remote_teardown_names_the_host_command() {
    let hits = decide(
        "ssh -o BatchMode=yes root@gex45 'hostname; systemctl restart user@1000.service; echo rc $?'",
    );
    assert_eq!(
        hits,
        [Hit::Teardown {
            shape: "user-manager teardown",
            host: Some("gex45".into())
        }]
    );
    assert!(reason(&hits).contains("`firn host restart-user-manager gex45`"));
    assert!(reason(&decide("loginctl terminate-user tom")).contains(HOST_RESTART));
}

#[test]
fn cross_session_pkill_and_killall_by_pattern_or_name_are_refused() {
    for command in [
        "pkill -f '[s]afe-push --to main'",
        "pkill -f '[c]argo test'",
        "killall cargo",
        "pkill -f 'safe-push --to main'",
        "pkill -9 -f 'cargo test'",
        "pkill -TERM -f 'cargo test'",
        "sudo pkill -f 'cargo.*test'",
        "pkill -u tom cargo",
        "pkill cargo",
        "killall -9 cargo",
        "echo $(killall -s TERM cargo)",
    ] {
        let hits = decide(command);
        assert!(matches!(hits[..], [Hit::CrossSession { .. }]), "{command}: {hits:?}");
        let reason = reason(&hits);
        for alternative in ["`pkill --cgroup", "recorded pid", "`pkill -F pidfile`"] {
            assert!(reason.contains(alternative), "{command}: {alternative}");
        }
    }
    for command in [
        "pkill -F /tmp/task.pid",
        "pkill --pidfile=/tmp/task.pid",
        "pkill --cgroup app.slice cargo",
        "pkill -P 1234 cargo",
        "killall -l",
        "kill $(cat /tmp/task.pid)",
    ] {
        assert!(!denied(command), "allowed {command}");
    }
}


#[test]
fn background_jobs_joined_by_a_wait_or_run_bounded_in_a_group_are_allowed() {
    for command in [
        "sleep 5 & wait",
        "sleep 5 & p=$!; sleep 1; wait $p; echo done",
        "for h in a b; do (ssh $h 'uptime') & done; wait",
        "( north capacity bounded 1h -- cargo test; echo ok ) &",
    ] {
        assert!(!denied(command), "{command}");
    }
    for command in [
        "sleep 5 &",
        "sleep 5 & echo started",
        "wait; sleep 5 &",
        "sleep 5 & wait; nohup sleep 9 &",
        "sleep 5 & setsid sleep 9; wait",
        "sleep 5 & disown; wait",
        "( cargo test; echo ok ) &",
    ] {
        assert!(denied(command), "{command}");
    }
}

#[test]
fn a_self_matching_pgrep_is_refused_only_when_its_output_is_consumed() {
    for command in [
        "pgrep -af 'cargo test' | head -3",
        "P=$(pgrep -f 'wisp.ts'); ls /proc/$P",
        "git log -1; pgrep -fa 'frontier oracle' | cut -c1-120; ls",
    ] {
        assert!(!denied(command), "{command}");
    }
    for command in [
        "until ! pgrep -f 'cargo test' >/dev/null; do sleep 1; done",
        "timeout 110 tail --pid=$(pgrep -f 'bash bin/safe-push.test.sh' | head -1) -f /dev/null",
        "for p in $(pgrep -f 'cargo test'); do kill $p; done",
        "pkill -f 'cargo build'; pgrep -fa 'safe-push' | head",
        "pkill -f 'cargo build'",
    ] {
        assert!(denied(command), "{command}");
    }
    let script = |wait: &str, file: &str| format!("cat > /tmp/{file}.sh <<'EOF'\n{wait}\ncd /tmp\necho done\nEOF\nbash /tmp/check.sh > /tmp/check.log 2>&1; cat /tmp/check.log");
    let waiting = "while ! [ -f /tmp/w/mobile_sam.pt ] || pgrep -f \"pip install\" >/dev/null; do sleep 5; done";
    let hits = decide(&script(waiting, "check"));
    assert!(matches!(&hits[..], [Hit::SelfMatch { tool: "pgrep", pattern }] if pattern == "pip install"), "{hits:?}");
    assert!(!denied(&script("pgrep -fa 'pip install' | head -3", "check")));
    assert!(!denied(&script(waiting, "other")));
}

#[test]
fn a_foreground_bun_run_of_a_tmp_script_runs_as_written() {
    let verdict = |command: &str| {
        let call = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command}});
        evaluate_with(&HookInput::parse(call.to_string().as_bytes()).unwrap(), true)
    };
    for command in [
        "bun /tmp/edit.ts; echo rc=$?; git diff --stat",
        "node /tmp/edit.mjs",
        "cd lane && bun /tmp/edit.ts src/a.ts | tail -3",
    ] {
        assert!(matches!(verdict(command), Verdict::Allow), "{command}");
    }
    for command in ["bun /tmp/edit.ts &", "nohup node /tmp/edit.mjs"] {
        assert!(matches!(verdict(command), Verdict::Rewrite(..)), "{command}");
    }
}

#[test]
fn a_background_job_without_a_user_bus_is_refused_with_the_run_in_background_instruction() {
    for command in ["bun /tmp/edit.ts &", "nohup node /tmp/edit.mjs", "sleep 5 &"] {
        let call = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command}});
        let input = HookInput::parse(call.to_string().as_bytes()).unwrap();
        assert!(matches!(evaluate_with(&input, true), Verdict::Rewrite(..)), "{command}");
        match evaluate_with(&input, false) {
            Verdict::Deny(reason) => assert!(reason.contains("run_in_background: true"), "{command}: {reason}"),
            _ => panic!("{command}"),
        }
    }
}
