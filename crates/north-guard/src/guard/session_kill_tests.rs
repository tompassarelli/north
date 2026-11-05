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

