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
        "sh -c id", "rsync --server -e.LsfxC . x", "north capacity nexus -- id", "internal-sftp", "threads record-priors",
        "threads agent-event x", "threads label-runs", "threads calibrate --write", "threads annotate-runs",
        "threads routing-compare", "threads snapshot-pull", "threads", "north fleet laptop open", "north fleet laptop close",
        "north fleet laptop drain", "north fleet laptop toggle", "north fleet laptop tick", "north runtime update", "north runtime deploy",
        "scp x y", "nix-store --serve", "north fleet row delete", "north fleet row leased", "north fleet usage",
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
        plan("env 'LAPTOP_BY=$(id)' north fleet laptop status"),
        Err(Refusal::EnvValue("LAPTOP_BY".into()))
    );
    for command in [
        "env LAPTOP_GRACE=1234567 north fleet laptop status", "env LAPTOP_GRACE=-1 north fleet laptop status",
        "env LAPTOP_GRACE= north fleet laptop status", "env THREADS_ACTOR= threads list",
        "env 'THREADS_ALIVE_OWNERS=[1]' threads list", "env 'THREADS_ALIVE_OWNERS=[\"a b\"]' threads list",
        "env THREADS_ALIVE_OWNERS=nope threads list",
    ] {
        assert!(matches!(plan(command), Err(Refusal::EnvValue(_))), "{command}");
    }
    assert_eq!(
        plan("env THREADS_ACTOR=a THREADS_ACTOR=b threads list"),
        Err(Refusal::EnvRepeated("THREADS_ACTOR".into()))
    );
    let long = format!("env THREADS_ACTOR={} threads list", "a".repeat(256));
    assert!(matches!(plan(&long), Err(Refusal::EnvValue(_))));
    let box_named = "north-1041-d0648-each-guarantee-a-hook-enforces-for-claude-today";
    let owners = format!("env 'THREADS_ALIVE_OWNERS=[\"{box_named}\"]' THREADS_ACTOR={box_named} threads list");
    assert!(plan(&owners).is_ok(), "a box-named session longer than 64 characters claims through remote-serve: {owners}");

}

#[test]
fn no_native_is_forced_whatever_the_caller_sends() {
    for command in ["threads list", "env THREADS_ACTOR=w1 threads list", "env THREADS_NO_NATIVE=0 threads list", "env THREADS_NO_NATIVE=1 threads list"] {
        assert_eq!(env_of(command)["THREADS_NO_NATIVE"], "1", "{command}");
    }
}


#[test]
fn nexus_reaches_a_worker_only_through_its_own_verb_set_and_a_threads_only_key_reaches_only_threads() {
    use super::{Grants, Job, Jobs, NEXUS_PEER, plan_for};
    struct NoJobs;
    impl Jobs for NoJobs {
        fn job(&self, _: i64) -> Option<Job> {
            None
        }
    }
    let nexus = Grants { peer: NEXUS_PEER.into(), repos: vec![], threads_only: false };
    for allowed in ["north fleet spool cache", "north fleet spool registry", "north fleet list --json", "north dash --closed --since 30d", "north runtime health", "north session-attach /home/w/.local/state/north/held/u1 u1 0a1b 0", "north session-attach /home/w/.local/state/north/held/u1 u1 0a1b 4096"] {
        assert!(plan_for(allowed, &nexus, &NoJobs).is_ok(), "{allowed}");
    }
    for refused in ["sh -s", "bash -s", "sh -c id", "north fleet spool", "north dash --closed --since 3w", "threads claim north#1", "south land", "cat x", "north session-attach /tmp/x u1 n 0", "north session-attach /home/w/.local/state/north/held/../../x u1 n 0", "north session-attach /home/w/.local/state/north/held/u1 u1 n 1x", "north session-attach /home/w/.local/state/north/held/u1 -x n 0"] {
        assert!(plan_for(refused, &nexus, &NoJobs).is_err(), "{refused}");
    }
    assert!(plan_for("north fleet spool cache", &Grants { peer: "w1".into(), repos: vec![], threads_only: false }, &NoJobs).is_err());
    let broker = Grants { peer: "cpx62".into(), repos: vec!["north".into()], threads_only: true };
    for allowed in ["threads list", "north threads show north#1", "env THREADS_ACTOR=w1 threads claim north#1"] {
        assert!(plan_for(allowed, &broker, &NoJobs).is_ok(), "{allowed}");
    }
    for refused in ["north fleet row put --peer cpx62", "north fleet row register", "north fleet dispatch needs north#1", "south land", "north runtime health", "north fleet laptop status", "threads show wisp#1"] {
        assert!(plan_for(refused, &broker, &NoJobs).is_err(), "a threads-only key ran {refused}");
    }
}
