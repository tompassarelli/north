use std::fs;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use north_core::fleet::store::{FarSpools, FleetRow, OverSsh, RowKind, Spool};
use north_core::foundation::shim;

struct Hosts {
    root: tempfile::TempDir,
}

impl Hosts {
    fn new() -> Self {
        let hosts = Self { root: tempfile::tempdir().unwrap() };
        let bin = hosts.path("bin");
        fs::create_dir_all(&bin).unwrap();
        symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        fs::create_dir_all(hosts.path("far")).unwrap();
        let ssh = format!("while [ \"$1\" != -- ]; do shift; done\nshift\nPATH=\"{}:$PATH\" HOME=\"{}\" exec sh -c \"$1\"\n", bin.display(), hosts.path("far").display());
        shim::write_script(&bin.join("ssh"), &ssh);
        hosts
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn spool(&self) -> Spool {
        Spool::at(&self.path("far").join(".local/state/north/fleet"))
    }

    fn rejected(&self) -> PathBuf {
        self.path("far").join(".local/state/north/fleet/spool/rejected")
    }

    fn ssh(&self) -> OverSsh {
        OverSsh::through(self.path("bin").join("ssh"))
    }

    fn north(&self, args: &[&str], input: &str) -> Output {
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap());
        let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["fleet", "spool"])
            .args(args)
            .env("HOME", self.path("far"))
            .env("PATH", path)
            .env_remove("NORTH_FLEET_SESSION")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }
}

fn message(id: &str) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: "sc-1".into(), from: "far".into(), urgent: false, sent_at: 1_793_000_000_000, text: "hello".into(), sender: None, gate: None, answer: None, job: None, origin: None }
}

fn taken(output: &Output) -> Vec<String> {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let spooled: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    spooled.iter().map(|spooled| spooled["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn take_lists_the_spooled_rows_in_id_order_and_changes_nothing_and_drop_removes_exactly_the_ids_on_stdin() {
    let hosts = Hosts::new();
    assert!(taken(&hosts.north(&["take"], "")).is_empty());

    ["m-2", "m-3", "m-1"].iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    assert_eq!(taken(&hosts.north(&["take"], "")), ["m-1", "m-2", "m-3"]);
    assert_eq!(taken(&hosts.north(&["take"], "")), ["m-1", "m-2", "m-3"]);

    let dropped = hosts.north(&["drop"], "m-1\nm-9\nm-3\n");
    assert!(dropped.status.success(), "{}", String::from_utf8_lossy(&dropped.stderr));
    assert_eq!(taken(&hosts.north(&["take"], "")), ["m-2"]);
}

#[test]
fn drop_refuses_a_path_like_id_and_removes_nothing() {
    let hosts = Hosts::new();
    hosts.spool().put(&message("m-1")).unwrap();

    let refused = hosts.north(&["drop"], "m-1\n../m-1\n");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a row id: ../m-1"), "{}", String::from_utf8_lossy(&refused.stderr));
    assert_eq!(taken(&hosts.north(&["take"], "")), ["m-1"]);
}

#[test]
fn the_ssh_client_takes_drops_and_sets_aside_through_the_real_over_ssh_quoting() {
    let hosts = Hosts::new();
    ["m-2", "m-1"].iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    let ssh = hosts.ssh();

    assert_eq!(ssh.take("far").unwrap().rows, [message("m-1"), message("m-2")]);
    ssh.drop("far", &["m-1".to_owned()]).unwrap();
    assert_eq!(ssh.take("far").unwrap().rows, [message("m-2")]);
    assert!(ssh.drop("far", &["../m-2".to_owned()]).is_err());
    assert_eq!(ssh.take("far").unwrap().rows, [message("m-2")]);
    ssh.set_aside("far", &["m-2".to_owned()]).unwrap();
    assert!(ssh.take("far").unwrap().rows.is_empty(), "a row set aside is never taken again");
    assert!(hosts.rejected().join("m-2.json").is_file());
}

#[test]
fn a_far_spool_sets_aside_only_rows_that_are_not_json_or_have_no_id_and_leaves_a_row_of_an_unknown_kind_for_a_build_that_knows_it() {
    let hosts = Hosts::new();
    ["m-1", "m-3"].iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    let dir = hosts.path("far").join(".local/state/north/fleet/spool");
    fs::write(dir.join("m-2.json"), b"{not json").unwrap();
    fs::write(dir.join("m-4.json"), br#"{"kind":"ANSWER","to":"sc-1"}"#).unwrap();
    fs::write(dir.join("m-5.json"), br#"{"id":"m-5","kind":"ANSWER-V2","to":"sc-1"}"#).unwrap();
    let ssh = hosts.ssh();

    let pull = ssh.take("far").unwrap();

    assert_eq!(pull.rows, [message("m-1"), message("m-3")]);
    let ids: Vec<&str> = pull.undecodable.iter().map(|undecodable| undecodable.id.as_str()).collect();
    assert_eq!(ids, ["m-2", "m-4"]);
    let [newer] = pull.unknown_kind.as_slice() else { panic!("{pull:?}") };
    assert_eq!((newer.id.as_str(), newer.kind.as_str()), ("m-5", "ANSWER-V2"));
    ssh.set_aside("far", &["m-2".to_owned(), "m-4".to_owned()]).unwrap();
    let again = ssh.take("far").unwrap();
    assert_eq!((again.rows.len(), again.undecodable.len(), again.unknown_kind.len()), (2, 0, 1));
    assert!(hosts.rejected().join("m-2.json").is_file() && hosts.rejected().join("m-4.json").is_file());
    assert!(!hosts.rejected().join("m-5.json").exists() && dir.join("m-5.json").is_file());
    fs::write(dir.join("m-5.json"), serde_json::to_vec(&message("m-5")).unwrap()).unwrap();
    assert_eq!(ssh.take("far").unwrap().rows, [message("m-1"), message("m-3"), message("m-5")]);
}

#[test]
fn five_thousand_ids_are_dropped_and_set_aside_over_ssh_without_one_argv_string() {
    let hosts = Hosts::new();
    let ids: Vec<String> = (0..5000).map(|n| format!("1793000000000-{n:05}")).collect();
    let ssh = hosts.ssh();

    ids.iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    ssh.drop("far", &ids).unwrap();
    assert!(ssh.take("far").unwrap().rows.is_empty());

    ids.iter().for_each(|id| hosts.spool().put(&message(id)).unwrap());
    ssh.set_aside("far", &ids).unwrap();
    assert!(ssh.take("far").unwrap().rows.is_empty());
    assert_eq!(fs::read_dir(hosts.rejected()).unwrap().count(), 5000);
}
