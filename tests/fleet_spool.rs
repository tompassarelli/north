use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output};

use north_core::error::NorthError;
use north_core::fleet::store::{FleetRow, RowKind, Spool, drop_with, take_with};

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
        fs::create_dir_all(hosts.path("far")).unwrap();
        hosts
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn spool(&self) -> Spool {
        Spool::at(&self.path("far").join(".local/state/north/fleet"))
    }

    fn north(&self, args: &[&str]) -> Output {
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["fleet", "spool"])
            .args(args)
            .env("HOME", self.path("far"))
            .env("PATH", path)
            .env_remove("NORTH_FLEET_SESSION")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .output()
            .unwrap()
    }
}

fn message(id: &str) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: "sc-1".into(), from: "far".into(), urgent: false, sent_at: 1_793_000_000_000, text: "hello".into(), sender: None, gate: None, job: None }
}

fn taken(output: &Output) -> Vec<FleetRow> {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn take_prints_the_spooled_rows_in_id_order_and_changes_nothing_and_drop_removes_exactly_the_named_rows() {
    let hosts = Hosts::new();
    assert_eq!(taken(&hosts.north(&["take"])), []);

    ["m-2", "m-3", "m-1"].iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    let expected = [message("m-1"), message("m-2"), message("m-3")];
    assert_eq!(taken(&hosts.north(&["take"])), expected);
    assert_eq!(taken(&hosts.north(&["take"])), expected);

    let dropped = hosts.north(&["drop", "m-1", "m-9", "m-3"]);
    assert!(dropped.status.success(), "{}", String::from_utf8_lossy(&dropped.stderr));
    assert_eq!(taken(&hosts.north(&["take"])), [message("m-2")]);
}

#[test]
fn drop_refuses_a_path_like_id_and_removes_nothing() {
    let hosts = Hosts::new();
    hosts.spool().put(&message("m-1")).unwrap();

    let refused = hosts.north(&["drop", "m-1", "../m-1"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a row id: ../m-1"), "{}", String::from_utf8_lossy(&refused.stderr));
    assert_eq!(taken(&hosts.north(&["take"])), [message("m-1")]);
}

#[test]
fn the_ssh_client_takes_and_drops_a_far_hosts_spool() {
    let hosts = Hosts::new();
    ["m-2", "m-1"].iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    let ssh = |words: &[&str]| {
        let command = format!("north {}", words.iter().map(|word| format!("'{word}'")).collect::<Vec<_>>().join(" "));
        let path = format!("{}:{}", hosts.path("bin").display(), std::env::var("PATH").unwrap());
        let output = Command::new(hosts.path("bin").join("ssh")).args(["-T", "far", "--", &command]).env("PATH", path).env("FAR_HOME", hosts.path("far")).output().unwrap();
        if output.status.success() { Ok(output.stdout) } else { Err(NorthError::Usage(String::from_utf8_lossy(&output.stderr).into_owned())) }
    };

    assert_eq!(take_with(ssh).unwrap(), [message("m-1"), message("m-2")]);
    drop_with(ssh, &["m-1".to_owned()]).unwrap();
    assert_eq!(take_with(ssh).unwrap(), [message("m-2")]);
    assert!(drop_with(ssh, &["../m-2".to_owned()]).is_err());
    assert_eq!(take_with(ssh).unwrap(), [message("m-2")]);
}
