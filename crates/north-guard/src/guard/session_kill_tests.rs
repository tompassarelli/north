use super::*;

fn denied(command: &str) -> bool {
    !decide(command).is_empty()
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

