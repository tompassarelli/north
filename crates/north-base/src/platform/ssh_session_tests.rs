use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn the_remote_command_after_ssh_options_and_the_destination_is_wrapped_in_the_session_scope() {
    let wrapped = scoped(&words(&["-p", "2222", "-oBatchMode=yes", "-tt", "gex45", "nproc;", "echo", "it's"]), "n-626", "0a1b2c3d");
    assert_eq!(wrapped[..4], words(&["-p", "2222", "-oBatchMode=yes", "-tt"]));
    assert_eq!(wrapped[4], "gex45");
    assert_eq!(wrapped.len(), 6);
    assert!(wrapped[5].contains("--unit=north-ssh-n-626-0a1b2c3d.scope") && wrapped[5].contains(r"q='nproc; echo it'\''s';"), "{}", wrapped[5]);
    assert_eq!(scope_session("north-ssh-n-626-0a1b2c3d.scope"), Some("n-626"));
}

#[test]
fn logins_control_calls_git_transport_and_north_protocol_pass_through_unchanged() {
    for call in [&["gex45"][..], &["-N", "-L", "8080:localhost:80", "gex45"], &["-O", "check", "gex45"], &["git@github.com", "git-upload-pack 'tompassarelli/north'"], &["-l", "tom", "gex45"], &["root@ccx33", "-x", "-oPermitLocalCommand=yes", "-oLocalCommand=echo started", "--", "nix-daemon", "--stdio"], &["root@ccx33", "-x", "--", "nix-store", "--serve", "--write"], &["-O", "check", "root@ccx33", "-oPermitLocalCommand=yes"], &["nexus", "threads queue take --host nexus"], &["gex45", "/opt/north/bin/north", "remote-serve"], &["nexus", "env 'THREADS_ALIVE_OWNERS=[\"a\", \"b\"]' THREADS_ACTOR=w1 north threads holder 'north#562'"]] {
        let call = words(call);
        assert_eq!(scoped(&call, "n-626", "0a1b2c3d"), call);
    }
    let call = words(&["-o", "BatchMode=yes", "gex45", "nproc"]);
    assert_eq!(refusal(&call, false), None, "a session whose sandbox keeps its hosts runs its ssh");
    assert!(refusal(&call, true).is_some_and(|told| told.contains("north capacity offload run --host HOST")), "a command whose sandbox hides every other host is told the verb that reaches one (south#118)");
    assert_eq!(refusal(&words(&["-V"]), true), None);
}

#[test]
fn a_script_after_the_destination_and_double_dash_is_wrapped_without_the_dash_or_environment_expansion() {
    let script = r#"if [ "${command#nix develop}" = "$command" ]; then echo hi; fi"#;
    let wrapped = scoped(&words(&["-T", "ccx33", "--", "setsid -w sh -c 'x'", script]), "n-626", "0a1b2c3d");
    assert_eq!(wrapped[..3], words(&["-T", "ccx33", "--"]));
    assert_eq!(wrapped.len(), 4);
    assert!(wrapped[3].contains("--expand-environment=no"), "{}", wrapped[3]);
    assert!(wrapped[3].contains("q='setsid -w sh -c "), "{}", wrapped[3]);
    assert!(!wrapped[3].contains("q='-- "), "{}", wrapped[3]);
}

#[test]
fn options_after_the_destination_stay_ssh_options_and_only_the_command_is_wrapped() {
    let wrapped = scoped(&words(&["root@ccx33", "-x", "-p", "2222", "nproc"]), "n-685", "0a1b2c3d");
    assert_eq!(wrapped[..4], words(&["root@ccx33", "-x", "-p", "2222"]));
    assert_eq!(wrapped.len(), 5);
    assert!(wrapped[4].contains("q='nproc';"), "{}", wrapped[4]);
}

#[test]
fn a_remote_command_of_100_kb_reaches_the_far_shell_inside_the_scoped_call() {
    let payload = "x".repeat(100 * 1024);
    let wrapped = scoped(&words(&["-T", "gex45", "--", "printf", "%s", &payload]), "n-951", "0a1b2c3d");
    let output = std::process::Command::new("sh").args(["-c", wrapped.last().unwrap()]).env_remove("XDG_RUNTIME_DIR").env("SHELL", "/bin/sh").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, payload.as_bytes());
}
