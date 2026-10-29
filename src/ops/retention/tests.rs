use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use super::family::Family;
use super::remote::Remote;
use super::{retain, Policy};

const HOUR: u64 = 3600;

struct Local;

impl Remote for Local {
    fn command(&self, script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }
}

struct Rig {
    state: tempfile::TempDir,
    archive: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let rig = Self { state: tempfile::tempdir().unwrap(), archive: tempfile::tempdir().unwrap() };
        fs::create_dir(rig.root()).unwrap();
        rig
    }

    fn root(&self) -> std::path::PathBuf {
        self.state.path().join("evidence")
    }

    fn entry(&self, name: &str, age_hours: u64, pinned: bool) {
        let directory = self.root().join(name);
        fs::create_dir_all(directory.join("shots")).unwrap();
        fs::write(directory.join("shots/a.png"), name).unwrap();
        fs::write(directory.join("log.txt"), "log").unwrap();
        if pinned {
            fs::write(directory.join("PINNED"), "").unwrap();
        }
        let then = SystemTime::now() - Duration::from_secs(age_hours * HOUR);
        for path in [directory.join("shots/a.png"), directory.join("log.txt"), directory.join("PINNED"), directory.join("shots"), directory.clone()] {
            if let Ok(file) = fs::File::open(&path) {
                file.set_modified(then).unwrap();
            }
        }
    }

    fn run(&self, dry_run: bool) -> bool {
        let family = Family { name: "evidence", base: format!("'{}'", self.state.path().display()), dir: "evidence", max_age: Duration::from_secs(72 * HOUR) };
        let policy = Policy { dry_run, host: String::new(), archive: self.archive.path().to_owned() };
        retain(&Local, &family, &policy, super::now())
    }

    fn here(&self, name: &str) -> bool {
        self.root().join(name).exists()
    }

    fn archived(&self, name: &str) -> bool {
        self.archive.path().join("evidence").join(name).join("shots/a.png").is_file()
    }
}

fn hold(directory: &Path) -> Child {
    Command::new("sleep").arg("60").current_dir(directory).spawn().unwrap()
}

#[test]
fn an_expired_entry_is_archived_with_matching_content_and_its_source_deleted() {
    let rig = Rig::new();
    rig.entry("old", 100, false);
    assert!(rig.run(false));
    assert!(!rig.here("old"));
    assert_eq!(fs::read_to_string(rig.archive.path().join("evidence/old/shots/a.png")).unwrap(), "old");
}

#[test]
fn pinned_recent_and_dry_run_entries_stay_on_the_source() {
    let rig = Rig::new();
    rig.entry("pinned", 500, true);
    rig.entry("fresh", 1, false);
    assert!(rig.run(false));
    assert!(rig.here("pinned") && rig.here("fresh"));
    assert!(!rig.archived("pinned") && !rig.archived("fresh"));
    rig.entry("expired", 100, false);
    assert!(rig.run(true));
    assert!(rig.here("expired") && !rig.archived("expired"));
}

#[test]
fn an_entry_held_open_by_a_process_is_kept_until_it_lets_go() {
    let rig = Rig::new();
    rig.entry("running", 100, false);
    let mut holder = hold(&rig.root().join("running"));
    assert!(rig.run(false));
    assert!(rig.here("running") && !rig.archived("running"));
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(rig.run(false));
    assert!(!rig.here("running") && rig.archived("running"));
}

#[test]
fn a_differing_copy_already_in_the_archive_blocks_the_delete() {
    let rig = Rig::new();
    rig.entry("clash", 100, false);
    let existing = rig.archive.path().join("evidence/clash");
    fs::create_dir_all(existing.join("shots")).unwrap();
    fs::write(existing.join("shots/a.png"), "different").unwrap();
    assert!(!rig.run(false));
    assert!(rig.here("clash"));
}

fn aged(directory: &Path, inner: &str) {
    let touch = |flag: &str, path: std::path::PathBuf| assert!(Command::new("touch").args([flag, "-d", "100 hours ago"]).arg(path).status().unwrap().success());
    touch("-h", directory.join(inner));
    touch("-c", directory.to_owned());
}

#[test]
fn an_entry_holding_a_link_is_archived_with_the_link_target_kept() {
    let rig = Rig::new();
    rig.entry("linked", 100, false);
    let directory = rig.root().join("linked");
    std::os::unix::fs::symlink("log.txt", directory.join("alias")).unwrap();
    aged(&directory, "alias");
    assert!(rig.run(false));
    assert!(!rig.here("linked") && rig.archived("linked"));
    assert_eq!(fs::read_link(rig.archive.path().join("evidence/linked/alias")).unwrap(), Path::new("log.txt"));
}

#[test]
fn an_archived_link_with_a_different_target_blocks_the_delete() {
    let rig = Rig::new();
    rig.entry("relinked", 100, false);
    let directory = rig.root().join("relinked");
    std::os::unix::fs::symlink("log.txt", directory.join("alias")).unwrap();
    aged(&directory, "alias");
    let existing = rig.archive.path().join("evidence/relinked");
    fs::create_dir_all(existing.join("shots")).unwrap();
    fs::write(existing.join("shots/a.png"), "relinked").unwrap();
    fs::write(existing.join("log.txt"), "log").unwrap();
    std::os::unix::fs::symlink("shots/a.png", existing.join("alias")).unwrap();
    assert!(!rig.run(false));
    assert!(rig.here("relinked"));
}

#[test]
fn an_entry_holding_a_fifo_is_not_deleted_for_want_of_a_checkable_copy() {
    let rig = Rig::new();
    rig.entry("piped", 100, false);
    let directory = rig.root().join("piped");
    assert!(Command::new("mkfifo").arg(directory.join("pipe")).status().unwrap().success());
    aged(&directory, "pipe");
    assert!(rig.run(false));
    assert!(rig.here("piped") && !rig.archived("piped"));
}
