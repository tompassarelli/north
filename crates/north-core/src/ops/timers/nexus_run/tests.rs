use std::path::Path;
use std::time::{Duration, Instant};

use super::plan::{Options, Parsed, launch_words, parse, select_files, shell_join, ssh_argv, stop_words};
use super::{baseline_for, exec};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn options(arguments: &[&str]) -> Options {
    match parse(&words(arguments)) {
        Ok(Parsed::Run(options)) => options,
        _ => panic!("arguments should parse"),
    }
}

#[test]
fn arguments_keep_installs_pulls_the_timeout_and_the_command_after_the_separator() {
    let parsed = options(&["--cwd", "ts", "--install", "a", "--install=b/c", "--pull", "out/x.tsv", "--output-dir", "/tmp/o", "--timeout-seconds", "30", "--", "bun", "run", "check"]);
    assert_eq!(parsed.cwd.as_deref(), Some("ts"));
    assert_eq!(parsed.install, ["a", "b/c"]);
    assert_eq!(parsed.pull, ["out/x.tsv"]);
    assert_eq!(parsed.timeout_seconds, 30);
    assert_eq!(parsed.command, ["bun", "run", "check"]);
    assert_eq!(options(&["true"]).timeout_seconds, 900);
}

#[test]
fn a_missing_command_a_timeout_out_of_range_and_paths_leaving_the_tree_are_refused() {
    let refused = |arguments: &[&str]| parse(&words(arguments)).err();
    assert!(refused(&[]).is_some_and(|message| message.contains("provide -- COMMAND")));
    assert!(refused(&["--timeout-seconds", "0", "--", "true"]).is_some());
    assert!(refused(&["--timeout-seconds", "3601", "--", "true"]).is_some());
    assert!(refused(&["--cwd", "/abs", "--", "true"]).is_some_and(|message| message.contains("relative")));
    assert!(refused(&["--install", "../x", "--", "true"]).is_some_and(|message| message.contains("relative")));
    assert!(matches!(parse(&words(&["--help"])), Ok(Parsed::Help)));
}

#[test]
fn only_tracked_and_unignored_files_that_exist_and_are_not_private_are_sent() {
    let source = tempfile::tempdir().expect("tempdir");
    for name in [".gitignore", "a.txt", "dir/b.txt", "new.txt", "sp ace.txt", "sub/s.txt", ".env", ".env.local", "node_modules/x/y.js", ".ssh/key", "sub/.env.d/z", "sub/.git/config"] {
        let path = source.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, "x").expect("file");
    }
    let listing = b"sub/s.txt\0a.txt\0.env\0.env.local\0node_modules/x/y.js\0.ssh/key\0sub/.env.d/z\0sub/.git/config\0gone.txt\0sp ace.txt\0a.txt\0dir/b.txt\0new.txt\0.gitignore\0";
    let selected: Vec<String> = select_files(listing, source.path()).iter().map(|name| String::from_utf8_lossy(name).into_owned()).collect();
    assert_eq!(selected, [".gitignore", "a.txt", "dir/b.txt", "new.txt", "sp ace.txt", "sub/s.txt"]);
}

#[test]
fn the_baseline_is_the_same_project_main_on_nexus_or_the_source_path_outside_code() {
    let home = Path::new("/home/tom");
    assert_eq!(baseline_for(Path::new("/home/tom/code/north/worktrees/x"), home, "/home/remote"), "/home/remote/code/north/main");
    assert_eq!(baseline_for(Path::new("/tmp/elsewhere/repo"), home, "/home/remote"), "/tmp/elsewhere/repo");
}

#[test]
fn ssh_reuses_one_connection_unless_the_caller_names_multiplexing() {
    let env = |name: &str| (name == "XDG_RUNTIME_DIR").then(|| "/run/user/4242".to_owned());
    let home = Path::new("/nonexistent-home");
    let reused = ssh_argv(&env, home, &["-o", "BatchMode=yes", "nexus", "true"]);
    assert!(reused.contains(&"ControlPath=/run/user/4242/north-ssh-%C".to_owned()));
    assert_eq!(reused[reused.len() - 3..], ["BatchMode=yes", "nexus", "true"]);
    for explicit in [&["-o", "ControlMaster=no"][..], &["-oControlPath=none"], &["-S", "none"], &["-o", "controlpersist no"]] {
        let argv = ssh_argv(&env, home, &[explicit, &["nexus", "true"][..]].concat());
        assert!(!argv.iter().any(|arg| arg == "ControlMaster=auto"), "{explicit:?}");
    }
    let long = |name: &str| (name == "XDG_RUNTIME_DIR").then(|| format!("/{}", "a".repeat(36)));
    assert!(!ssh_argv(&long, home, &["nexus"]).iter().any(|arg| arg == "ControlMaster=auto"));
    let configured = |name: &str| (name == "NORTH_SSH_CONFIG").then(|| "/etc/ssh-config".to_owned());
    assert_eq!(ssh_argv(&configured, home, &["nexus"]), ["ssh", "-F", "/etc/ssh-config", "nexus"]);
}

#[test]
fn the_remote_launch_carries_the_pid_file_timeout_tree_working_directory_installs_and_command() {
    let parsed = options(&["--install", "ts", "--install", "tools", "--timeout-seconds", "60", "--", "bun", "run", "it's"]);
    let launch = launch_words("/r/job", "ts", &parsed);
    assert_eq!(launch[..2], ["sh", "-c"]);
    assert!(launch[2].contains("--owner nexus-run") && launch[2].contains("machine-capacity.mjs"));
    assert_eq!(launch[4..], ["/r/job/run.pid", "60", "/r/job/source", "ts", "2", "ts", "tools", "bun", "run", "it's"]);
    let joined = shell_join(&launch);
    assert!(joined.contains("'it'\"'\"'s'"));
    assert_eq!(stop_words("/r/job")[4], "/r/job");
}

#[test]
fn the_remote_job_runs_in_the_tree_with_the_output_directory_exported() {
    let root = tempfile::tempdir().expect("tempdir");
    let source = root.path().join("job/source");
    std::fs::create_dir_all(source.join("ts")).expect("tree");
    let script = super::plan::job_script();
    let probe = r#"printf '%s %s' "$PWD" "$NEXUS_OUTPUT""#;
    let output = std::process::Command::new("sh")
        .args(["-c", &script, "nexus-job"])
        .arg(&source)
        .args(["ts", "0", "sh", "-c", probe])
        .env("HOME", "/h")
        .output()
        .expect("run job");
    let expected = format!("{} {}", source.join("ts").display(), root.path().join("job/out").display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[tokio::test]
async fn a_stalled_command_is_killed_at_its_bound_instead_of_waiting() {
    let started = Instant::now();
    let result = exec(&words(&["sleep", "60"]), None, false, Duration::from_millis(100), true).await;
    assert!(result.is_err_and(|message| message.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(5));
}
