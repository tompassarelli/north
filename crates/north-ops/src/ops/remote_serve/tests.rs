use super::{Refusal, plan};

fn env_of(command: &str) -> std::collections::BTreeMap<String, String> {
    plan(command).unwrap().env
}

#[test]
fn lexer_refuses_shell_metacharacters_outside_quotes() {
    for command in [
        "threads list; id", "threads list && id", "threads list | cat", "threads list > /x", "threads list < /x",
        "threads list $(id)", "threads list `id`", "threads list $HOME", "threads list \\n", "threads list *",
        "threads list ~", "threads list {a,b}", "threads list #x", "threads list\nid", "threads list &", "(threads list)",
        "threads list !x", "threads list \"$HOME\"", "threads list \"`id`\"", "threads list \"a\\b\"",
    ] {
        assert!(matches!(plan(command), Err(Refusal::Metacharacter(_))), "{command}");
    }
}

#[test]
fn verbs_off_the_allowlist_are_refused() {
    for command in [
        "sh -c id", "rsync --server -e.LsfxC . x", "north nexus-run -- id", "internal-sftp", "threads record-priors",
        "threads agent-event x", "threads label-runs", "threads calibrate --write", "threads annotate-runs",
        "threads routing-compare", "threads snapshot-pull", "threads", "north laptop open", "north laptop close",
        "north laptop drain", "north laptop toggle", "north laptop tick", "north runtime update", "north runtime deploy",
        "scp x y", "nix-store --serve", "north fleet row delete", "north fleet row leased", "agents usage",
        "readlink /etc/passwd", "env north", "env -i threads list", "bash", "south land x",
    ] {
        assert!(matches!(plan(command), Err(Refusal::Verb(_))), "{command}");
    }
}

#[test]
fn env_prefix_refuses_unknown_keys_and_bad_values() {
    assert_eq!(plan("env LD_PRELOAD=/x threads list"), Err(Refusal::EnvKey("LD_PRELOAD".into())));
    assert_eq!(plan("env PATH=/x threads list"), Err(Refusal::EnvKey("PATH".into())));
    assert_eq!(
        plan("env 'LAPTOP_BY=$(id)' north laptop status"),
        Err(Refusal::EnvValue("LAPTOP_BY".into()))
    );
    for command in [
        "env LAPTOP_GRACE=1234567 north laptop status", "env LAPTOP_GRACE=-1 north laptop status",
        "env LAPTOP_GRACE= north laptop status", "env THREADS_ACTOR= threads list",
        "env 'THREADS_ALIVE_OWNERS=[1]' threads list", "env 'THREADS_ALIVE_OWNERS=[\"a b\"]' threads list",
        "env THREADS_ALIVE_OWNERS=nope threads list",
    ] {
        assert!(matches!(plan(command), Err(Refusal::EnvValue(_))), "{command}");
    }
    assert_eq!(
        plan("env THREADS_ACTOR=a THREADS_ACTOR=b threads list"),
        Err(Refusal::EnvRepeated("THREADS_ACTOR".into()))
    );
    let long = format!("env THREADS_ACTOR={} threads list", "a".repeat(65));
    assert!(matches!(plan(&long), Err(Refusal::EnvValue(_))));
}

#[test]
fn no_native_is_forced_whatever_the_caller_sends() {
    for command in ["threads list", "env THREADS_ACTOR=w1 threads list", "env THREADS_NO_NATIVE=0 threads list", "env THREADS_NO_NATIVE=1 threads list"] {
        assert_eq!(env_of(command)["THREADS_NO_NATIVE"], "1", "{command}");
    }
}

