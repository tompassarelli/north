//! A claim past its ETA stays held while its exact holder session is alive and frees when it stops,
//! here and through the ledger host.
use std::fs;
use north_core::foundation::shim;
use std::path::{Path, PathBuf};
use std::process::Command;

const GH: &str = r#"case "$*" in
  "issue list -R tompassarelli/makgora"*) printf '[{"number":10,"title":"Resume","labels":[{"name":"priority:now"}],"body":"- [ ] finish","comments":[]}]' ;;
  "issue list"*) printf '[]' ;;
  *refPrefix*) printf '{"data":{"repository":{"refs":{"nodes":[]}}}}' ;;
  *) printf '{"data":{"repository":{"issue":{"number":10,"title":"Resume","state":"OPEN","blockedBy":{"nodes":[]}}}}}' ;;
esac
"#;

const SSH: &str = r#"printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
HOME=$REMOTE_HOME THREADS_DB=$REMOTE_DB exec bash -c "${@: -1}"
"#;

fn bash(path: &Path, body: &str) {
    shim::write_executable(path, format!("#!/usr/bin/env bash\n{body}"));
}

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        let home = root.join("home");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("activation.active"), "hook north-hook\n").unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
        bash(&root.join("bin/gh"), GH);
        Rig { _scratch: scratch, root, home }
    }

    fn holder(&self, stopped: bool) {
        let directory = self.home.join(".local/state/north/fleet/holder");
        fs::create_dir_all(&directory).unwrap();
        let session = serde_json::json!({"name": "holder", "provider": "codex", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "fixture-session", "stopped": stopped});
        fs::write(directory.join("session.json"), format!("{session}\n")).unwrap();
    }

    fn threads(&self, args: &[&str], local: bool) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command.arg("threads").args(args)
            .env("HOME", &self.home)
            .env("NORTH_AGENT_ACTIVE", self.root.join("activation.active"))
            .env("NORTH_BIN", self.root.join("bin/north"))
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env("REMOTE_DB", self.root.join("remote.db"))
            .env("REMOTE_HOME", self.root.join("remote-home"))
            .env("SSH_TRACE", self.root.join("ssh.trace"))
            .env_remove("THREADS_ALIVE_OWNERS")
            .env_remove("THREADS_NO_NATIVE")
            .env_remove("CLAUDE_CODE_REMOTE")
            .env_remove("THREADS_ACTOR")
            .env_remove("XDG_STATE_HOME");
        if local {
            command.env("THREADS_DB", self.root.join("threads.db"));
        } else {
            command.env_remove("THREADS_DB");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

#[test]
fn an_expired_claim_stays_held_for_its_live_holder_here_and_through_the_ledger_host() {
    let rig = Rig::new();
    let held = "makgora#10\tpriority:now\tcodex:fixture-session\t";
    let free = "makgora#10\tpriority:now\tUNOWNED\t";
    rig.holder(false);
    rig.threads(&["claim", "makgora#10", "--by", "codex:fixture-session", "--eta", "5"], true);
    rusqlite::Connection::open(rig.root.join("threads.db")).unwrap()
        .execute_batch("UPDATE claims SET since = '2020-01-01T00:00:00Z'; UPDATE events SET ts = '2020-01-01T00:00:00Z'").unwrap();
    assert!(rig.threads(&["unowned"], true).contains(held));
    rig.holder(true);
    assert!(rig.threads(&["unowned"], true).contains(free));
    rig.holder(false);
    assert!(rig.threads(&["unowned"], true).contains(held));

    fs::create_dir_all(rig.home.join(".config/threads")).unwrap();
    fs::create_dir_all(rig.home.join(".local/state/threads")).unwrap();
    fs::create_dir_all(rig.root.join("remote-home")).unwrap();
    fs::write(rig.home.join(".config/threads/remote"), "fixture-host\n").unwrap();
    fs::copy(rig.root.join("threads.db"), rig.root.join("remote.db")).unwrap();
    bash(&rig.root.join("bin/ssh"), SSH);
    assert!(rig.threads(&["unowned"], false).contains(held));
    let trace = fs::read_to_string(rig.root.join("ssh.trace")).unwrap();
    assert!(trace.contains("THREADS_ALIVE_OWNERS=") && !trace.contains("pid"), "the proxy relays exact caller liveness without PIDs: {trace}");
    rig.holder(true);
    assert!(rig.threads(&["unowned"], false).contains(free));
}
