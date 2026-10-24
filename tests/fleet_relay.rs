use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
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

fn posted(home: &Path) -> Vec<PathBuf> {
    let outbox = home.join(".local/state/north/fleet/.outbox");
    let mut letters: Vec<PathBuf> = fs::read_dir(outbox).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "json")).collect();
    letters.sort();
    letters
}

#[test]
fn a_send_on_a_host_without_the_session_reaches_the_fleet_host_over_ssh_or_reports_once_why_not() {
    let hosts = Hosts::new();
    let alone = hosts.north("far", &["send", "proxy", "rolled", "back"]);
    assert!(!alone.status.success());
    assert!(text(&alone.stderr).contains("no session named proxy"), "{}", text(&alone.stderr));

    let first = hosts.north("fleet", &["relay", "far"]);
    assert!(first.status.success(), "{}{}", text(&first.stdout), text(&first.stderr));

    let sent = hosts.north("far", &["send", "proxy", "rolled", "back"]);
    assert!(sent.status.success(), "{}", text(&sent.stderr));
    assert!(text(&sent.stderr).contains("no session named proxy here; posted "), "{}", text(&sent.stderr));
    let letters = posted(&hosts.path("far"));
    assert_eq!(letters.len(), 1);
    let letter: serde_json::Value = serde_json::from_slice(&fs::read(&letters[0]).unwrap()).unwrap();
    assert_eq!((letter["to"].as_str(), letter["text"].as_str(), letter["now"].as_bool()), (Some("proxy"), Some("rolled back"), Some(false)));

    let missing = hosts.north("fleet", &["relay", "far"]);
    assert!(!missing.status.success(), "a letter the fleet host cannot deliver fails the relay");
    let id = letter["id"].as_str().unwrap();
    assert!(text(&missing.stdout).contains(&format!("undelivered {id} from far to proxy: no session named proxy")), "{}", text(&missing.stdout));
    assert!(text(&missing.stderr).contains(&format!("newly undelivered or undropped: far {id}")), "{}", text(&missing.stderr));

    let repeated = hosts.north("fleet", &["relay", "far"]);
    assert!(repeated.status.success(), "{}", text(&repeated.stderr));
    assert_eq!(posted(&hosts.path("far")), letters, "an undelivered letter stays where it was posted");
}
