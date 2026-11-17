use super::{Grants, NEXUS_PEER, Refusal, plan, plan_for};

#[test]
fn lexer_refuses_shell_metacharacters_outside_quotes() {
    for command in [
        "north runtime health; id", "north runtime health && id", "north runtime health | cat",
        "north runtime health > /x", "north runtime health < /x", "north runtime health $(id)",
        "north runtime health `id`", "north runtime health $HOME", "north runtime health \\n",
        "north runtime health *", "north runtime health ~", "north runtime health {a,b}",
        "north runtime health #x", "north runtime health\nid", "north runtime health &",
        "(north runtime health)", "north runtime health !x", "north runtime health \"$HOME\"",
        "north runtime health \"`id`\"", "north runtime health \"a\\b\"",
    ] {
        assert!(matches!(plan(command), Err(Refusal::Metacharacter(_))), "{command}");
    }
}

#[test]
fn verbs_off_the_allowlist_are_refused() {
    for command in [
        "sh -c id", "rsync --server -e.LsfxC . x", "north capacity nexus -- id", "internal-sftp",
        "threads list", "north threads show north#1", "north fleet list --json",
        "north fleet row put", "north github-mirror serve", "north runtime update",
        "north runtime deploy", "scp x y", "nix-store --serve", "readlink /etc/passwd",
        "env north", "env -i north runtime health", "bash", "south land x",
    ] {
        assert!(matches!(plan(command), Err(Refusal::Verb(_))), "{command}");
    }
}

#[test]
fn env_prefix_refuses_every_key() {
    for key in ["LD_PRELOAD", "PATH", "THREADS_ACTOR", "THREADS_NO_NATIVE", "LAPTOP_BY"] {
        assert_eq!(plan(&format!("env {key}=x north runtime health")), Err(Refusal::EnvKey(key.into())));
    }
}

#[test]
fn nexus_reaches_a_worker_only_through_its_own_verb_set() {
    let nexus = Grants { peer: NEXUS_PEER.into() };
    for allowed in ["north runtime health", "readlink .local/state/north/current"] {
        assert!(plan_for(allowed, &nexus).is_ok(), "{allowed}");
    }
    for refused in [
        "sh -s", "bash -s", "sh -c id", "south land", "north holds list --here --json",
        "north fleet spool cache", "threads list", "north session-attach /home/w/.local/state/north/held/u1 u1 0a1b 0",
    ] {
        assert!(plan_for(refused, &nexus).is_err(), "{refused}");
    }
    let peer = Grants { peer: "gex45".into() };
    for allowed in ["north runtime health", "readlink .local/state/north/current", "north holds list --here --json", "south land"] {
        assert!(plan_for(allowed, &peer).is_ok(), "{allowed}");
    }
}
