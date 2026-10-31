use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output};

const SSH: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nHOME=\"$FAR_HOME\" exec sh -c \"$1\"\n";

struct Hosts {
    root: tempfile::TempDir,
}

impl Hosts {
    fn new() -> Self {
        let hosts = Self { root: tempfile::tempdir().unwrap() };
        let bin = hosts.path("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("ssh"), SSH).unwrap();
        fs::set_permissions(bin.join("ssh"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        for home in ["far", "fleet"] {
            fs::create_dir_all(hosts.path(home)).unwrap();
        }
        hosts
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn north(&self, home: &str, args: &[&str]) -> Output {
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["fleet"])
            .args(args)
            .env("HOME", self.path(home))
            .env("FAR_HOME", self.path("far"))
            .env("PATH", path)
            .env_remove("NORTH_FLEET_SESSION")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_send_on_a_host_without_the_session_spools_for_a_name_the_fleet_table_holds_and_refuses_any_other_whatever_a_relay_reads() {
    let hosts = Hosts::new();
    let relayed = hosts.north("fleet", &["relay", "far"]);
    assert!(relayed.status.success(), "{}{}", text(&relayed.stdout), text(&relayed.stderr));

    let alone = hosts.north("far", &["send", "proxy", "rolled", "back"]);
    assert!(!alone.status.success());
    assert!(text(&alone.stderr).contains("no session named proxy"), "{}", text(&alone.stderr));

    let table = hosts.path("far").join(".local/state/north/fleet/.fleet-sessions.json");
    fs::write(&table, r#"[{"name":"proxy","host":"nexus","status":"idle","created":"t","updated":"t"}]"#).unwrap();
    let sent = hosts.north("far", &["send", "proxy", "rolled", "back"]);
    assert!(sent.status.success(), "{}", text(&sent.stderr));
    assert!(text(&sent.stderr).contains("proxy runs on nexus; spooled "), "{}", text(&sent.stderr));
    let spooled: Vec<serde_json::Value> = serde_json::from_slice(&hosts.north("far", &["spool", "take"]).stdout).unwrap();
    let rows: Vec<(&str, &str, &str)> = spooled.iter().map(|entry| &entry["row"]).map(|row| (row["kind"].as_str().unwrap(), row["to_node"].as_str().unwrap(), row["text"].as_str().unwrap())).collect();
    assert_eq!(rows, [("MESSAGE", "proxy", "rolled back")]);
    assert!(!hosts.path("far").join(".local/state/north/fleet/.outbox").exists(), "a send posts no letter");
}
