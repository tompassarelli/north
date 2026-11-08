use std::path::PathBuf;
use std::process::{Command, Output};

use north_base::verdict_cache::path_set::{self, Entry, Kind};

struct Scene {
    root: tempfile::TempDir,
}

impl Scene {
    fn new() -> Self {
        let scene = Self { root: tempfile::tempdir().unwrap() };
        std::fs::create_dir_all(scene.tree().join("dir")).unwrap();
        std::fs::write(scene.tree().join("a.txt"), "a\n").unwrap();
        std::fs::write(scene.tree().join("dir/inner.txt"), "inner\n").unwrap();
        std::fs::write(scene.outside(), "outside\n").unwrap();
        scene
    }

    fn tree(&self) -> PathBuf {
        self.root.path().join("tree")
    }

    fn outside(&self) -> PathBuf {
        self.root.path().join("outside.txt")
    }

    fn sandbox(&self) -> Command {
        let mut sandbox = Command::new(env!("CARGO_BIN_EXE_north-land-sandbox"));
        sandbox.args(["land", "sandbox", "exec", "--tree"]).arg(self.tree()).current_dir(self.tree()).env_remove("CARGO_TARGET_DIR").env_remove("LANG").env_remove("LC_ALL").env_remove("LANGUAGE").env_remove("LOCALE_ARCHIVE");
        sandbox
    }

    fn record(&self, script: &str) -> Vec<Entry> {
        let file = self.root.path().join("recorded");
        let output = self.sandbox().arg("--record").arg(&file).args(["--", "sh", "-c", script]).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        path_set::parse(&std::fs::read_to_string(file).unwrap()).unwrap()
    }
}

fn entry(kind: Kind, path: &str) -> Entry {
    Entry { kind, path: path.to_owned() }
}

fn unreusable_reasons(entries: &[Entry]) -> Vec<&str> {
    entries.iter().filter(|entry| entry.kind == Kind::Unreusable).map(|entry| entry.path.as_str()).collect()
}

#[test]
fn a_read_of_a_tree_file_yields_its_content() {
    let scene = Scene::new();
    let tree = scene.tree();
    let entries = scene.record(&format!("cat {}/a.txt", tree.display()));
    assert!(entries.contains(&entry(Kind::Content, "a.txt")), "{entries:?}");
}

#[test]
fn a_probe_of_a_missing_tree_path_yields_absent() {
    let scene = Scene::new();
    let tree = scene.tree();
    let entries = scene.record(&format!("test -e {}/missing.txt; true", tree.display()));
    assert!(entries.contains(&entry(Kind::Absent, "missing.txt")), "{entries:?}");
}

#[test]
fn a_listing_of_a_tree_directory_yields_listing() {
    let scene = Scene::new();
    let tree = scene.tree();
    let entries = scene.record(&format!("ls {}/dir", tree.display()));
    assert!(entries.contains(&entry(Kind::Listing, "dir")), "{entries:?}");
}

#[test]
fn a_read_under_tmpdir_and_of_a_file_the_command_made_yields_nothing() {
    let scene = Scene::new();
    let tree = scene.tree();
    let entries = scene.record(&format!("echo x > \"$TMPDIR/note\"; cat \"$TMPDIR/note\"; echo y > {0}/made.txt; cat {0}/made.txt", tree.display()));
    assert!(entries.iter().all(|entry| !entry.path.contains("note") && !entry.path.contains("made.txt")), "{entries:?}");
}

#[test]
fn a_read_outside_every_root_makes_the_set_unreusable_naming_the_path() {
    let scene = Scene::new();
    let outside = scene.outside();
    let entries = scene.record(&format!("cat {}", outside.display()));
    let reasons = unreusable_reasons(&entries);
    assert!(reasons.iter().any(|reason| reason.contains(outside.to_str().unwrap())), "{reasons:?}");
}

#[test]
fn a_statfs_makes_the_set_unreusable_naming_the_call() {
    let scene = Scene::new();
    let tree = scene.tree();
    let entries = scene.record(&format!("stat -f {}", tree.display()));
    assert!(unreusable_reasons(&entries).iter().any(|reason| reason.contains("statfs")), "{entries:?}");
}

#[test]
fn a_command_that_swallows_an_enforce_refusal_still_fails() {
    let scene = Scene::new();
    let listed = path_set::render(&[entry(Kind::Content, "a.txt")]);
    let file = scene.root.path().join("set");
    std::fs::write(&file, listed).unwrap();
    let outside = scene.outside();
    let output: Output = scene.sandbox().arg("--enforce").arg(&file).args(["--", "sh", "-c"]).arg(format!("cat {} 2>/dev/null; exit 0", outside.display())).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("sandbox refused read of {}", outside.display())), "{stderr}");
}

#[test]
fn a_child_stopped_by_sigstop_stays_stopped_until_sigcont() {
    let scene = Scene::new();
    let script = r#"
beat="$TMPDIR/beat"
exec 3>> "$beat"
(while :; do echo >&3; done) &
child=$!
sleep 0.2
kill -STOP $child
sleep 0.1
before=$(wc -l < "$beat")
sleep 0.3
during=$(wc -l < "$beat")
kill -CONT $child
sleep 0.3
after=$(wc -l < "$beat")
kill $child
[ "$before" = "$during" ] && [ "$after" -gt "$during" ] && echo stayed-stopped
"#;
    let file = scene.root.path().join("recorded");
    let output = scene.sandbox().arg("--record").arg(&file).args(["--", "sh", "-c", script]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "stayed-stopped", "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_child_stopped_by_sigstop_still_dies_of_sigterm() {
    let scene = Scene::new();
    let script = "sleep 5 & child=$!; sleep 0.2; kill -STOP $child; sleep 0.1; kill -TERM $child; wait $child; echo $?";
    let file = scene.root.path().join("recorded");
    let output = scene.sandbox().arg("--record").arg(&file).args(["--", "sh", "-c", script]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "143", "{}", String::from_utf8_lossy(&output.stderr));
}
