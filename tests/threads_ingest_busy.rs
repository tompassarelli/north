use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

const SSH: &str = "#!/bin/sh\nfor last; do :; done\nprintf '%s\\n' \"$last\" >> \"$HOME/ssh.log\"\ncase \"$last\" in *ingest) cat >/dev/null; exit 75 ;; esac\nexit 0\n";

fn threads(root: &Path, args: &[&str], stdin: &[u8]) -> std::process::Output {
    let path = format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let mut child = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"))
        .args(args)
        .env_remove("THREADS_DB")
        .env("HOME", root)
        .env("PATH", path)
        .env("NORTH_BIN", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn a_busy_ledger_host_spools_the_python_sender_batch_and_other_commands_still_reach_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join(".config/threads")).unwrap();
    fs::write(root.join(".config/threads/remote"), "nexus\n").unwrap();
    fs::write(root.join("bin/ssh"), SSH).unwrap();
    fs::set_permissions(root.join("bin/ssh"), fs::Permissions::from_mode(0o755)).unwrap();
    let spool = root.join(".local/state/threads/spool");
    let queued = || fs::read_dir(&spool).unwrap().filter_map(|entry| entry.ok()?.path().extension()?.to_str().map(str::to_owned)).collect::<Vec<_>>();

    let ingest = threads(root, &["ingest"], b"{\"type\":\"run\"}\n");
    assert!(ingest.status.success(), "{}", String::from_utf8_lossy(&ingest.stderr));
    assert_eq!(queued(), ["jsonl"]);

    let show = threads(root, &["show", "north#1"], b"");
    assert!(show.status.success(), "a busy ingest lock is not an unreachable host: {}", String::from_utf8_lossy(&show.stderr));
    assert_eq!(queued(), ["jsonl"], "the busy batch stays queued, never .failed");
    let log = fs::read_to_string(root.join("ssh.log")).unwrap();
    assert!(log.lines().last().unwrap().contains("threads show"), "{log}");
}
