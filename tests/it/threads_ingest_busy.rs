use north_core::foundation::shim;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const SSH: &str = "for last; do :; done\nprintf '%s\\n' \"$last\" >> \"$HOME/ssh.log\"\ncase \"$last\" in *ingest) cat >/dev/null; exit 75 ;; esac\nexit 0\n";
const NORTH: &str = "case \"$1\" in ssh-argv) exec \"$REAL_NORTH\" \"$@\" ;; esac\nexit 0\n";

fn threads(root: &Path, args: &[&str], stdin: &[u8]) -> std::process::Output {
    let path = format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .arg("threads")
        .args(args)
        .env_remove("THREADS_DB")
        .env("HOME", root)
        .env("PATH", path)
        .env_remove("XDG_RUNTIME_DIR")
        .env("NORTH_BIN", root.join("bin/north"))
        .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn a_busy_ledger_host_spools_the_sender_batch_and_other_commands_still_reach_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join(".config/threads")).unwrap();
    fs::write(root.join(".config/threads/remote"), "nexus\n").unwrap();
    shim::write_script(&root.join("bin/ssh"), SSH);
    shim::write_script(&root.join("bin/north"), NORTH);
    let spool = root.join(".local/state/threads/spool");
    let queued = || fs::read_dir(&spool).unwrap().filter_map(|entry| entry.ok()?.path().extension()?.to_str().map(str::to_owned)).collect::<Vec<_>>();

    let ingest = threads(root, &["ingest"], b"{\"type\":\"run\"}\n");
    assert!(ingest.status.success(), "{}", String::from_utf8_lossy(&ingest.stderr));
    assert_eq!(queued(), ["jsonl"]);

    let show = threads(root, &["show", "north#1"], b"");
    assert!(show.status.success(), "a busy ingest lock is not an unreachable host: {}", String::from_utf8_lossy(&show.stderr));
    assert_eq!(queued(), ["jsonl"], "the busy batch stays queued, never .failed");
    let log = fs::read_to_string(root.join("ssh.log")).unwrap();
    assert!(log.lines().any(|line| line.contains("north threads show")), "{log}");
}
