use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

// KEEP(contract): options with attached or separate values stay ahead of the destination, and only the remote command is wrapped
#[test]
fn the_remote_command_after_ssh_options_and_the_destination_is_wrapped_in_the_session_scope() {
    let wrapped = scoped(&words(&["-p", "2222", "-oBatchMode=yes", "-tt", "gex45", "nproc;", "echo", "it's"]), "n-626", "0a1b2c3d");
    assert_eq!(wrapped[..4], words(&["-p", "2222", "-oBatchMode=yes", "-tt"]));
    assert_eq!(wrapped[4], "gex45");
    assert_eq!(wrapped.len(), 6);
    assert!(wrapped[5].contains("--unit=north-ssh-n-626-0a1b2c3d.scope") && wrapped[5].contains(r"-c 'nproc; echo it'\''s'"), "{}", wrapped[5]);
    assert_eq!(scope_session("north-ssh-n-626-0a1b2c3d.scope"), Some("n-626"));
}

// KEEP(contract): north#654, systemd-run expanded ${...} in an offloaded command before the far shell saw it
#[test]
fn a_remote_commands_parameter_expansions_reach_the_far_shell_unexpanded() {
    let wrapped = scoped(&words(&["gex45", "t=\"${stamp%%-*}\"; echo \"${t#*-}\""]), "n-654", "0a1b2c3d");
    assert!(wrapped[1].contains("systemd-run --user --scope --quiet --collect --expand-environment=no --unit="), "{}", wrapped[1]);
    assert!(wrapped[1].contains(r"${stamp%%-*}") && wrapped[1].contains(r"${t#*-}"), "{}", wrapped[1]);
}

// KEEP(contract): logins, control and forwarding calls, git transport and the north protocol a peer's forced-command key parses reach ssh untouched
#[test]
fn logins_control_calls_git_transport_and_north_protocol_pass_through_unchanged() {
    for call in [&["gex45"][..], &["-N", "-L", "8080:localhost:80", "gex45"], &["-O", "check", "gex45"], &["git@github.com", "git-upload-pack 'tompassarelli/north'"], &["-l", "tom", "gex45"], &["nexus", "threads queue take --host nexus"], &["gex45", "/opt/north/bin/north", "remote-serve"], &["nexus", "env 'THREADS_ALIVE_OWNERS=[\"a\", \"b\"]' THREADS_ACTOR=w1 north threads holder 'north#562'"]] {
        let call = words(call);
        assert_eq!(scoped(&call, "n-626", "0a1b2c3d"), call);
    }
}

// KEEP(contract): a script after `ssh HOST --` reaches the far shell as the command, without the `--`, and systemd-run expands none of its `${...}` words (north#654)
#[test]
fn a_script_after_the_destination_and_double_dash_is_wrapped_without_the_dash_or_environment_expansion() {
    let script = r#"if [ "${command#nix develop}" = "$command" ]; then echo hi; fi"#;
    let wrapped = scoped(&words(&["-T", "ccx33", "--", "setsid -w sh -c 'x'", script]), "n-626", "0a1b2c3d");
    assert_eq!(wrapped[..2], words(&["-T", "ccx33"]));
    assert_eq!(wrapped.len(), 3);
    assert!(wrapped[2].contains("--expand-environment=no"), "{}", wrapped[2]);
    assert!(wrapped[2].contains("-c 'setsid -w sh -c "), "{}", wrapped[2]);
    assert!(!wrapped[2].contains("-c '-- "), "{}", wrapped[2]);
}
