use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use north_base::verdict_cache::path_set::{Entry, Kind, render};

struct Scene {
    root: tempfile::TempDir,
}

impl Scene {
    fn new() -> Self {
        let scene = Self { root: tempfile::tempdir().unwrap() };
        std::fs::create_dir(scene.tree()).unwrap();
        std::fs::write(scene.tree().join("listed.txt"), "listed\n").unwrap();
        std::fs::write(scene.tree().join("unlisted.txt"), "unlisted\n").unwrap();
        std::fs::write(scene.outside(), "outside\n").unwrap();
        let listed = Entry { kind: Kind::Content, path: "listed.txt".into() };
        std::fs::write(scene.path_set(), render(&[listed])).unwrap();
        scene
    }

    fn tree(&self) -> PathBuf {
        self.root.path().join("tree")
    }

    fn outside(&self) -> PathBuf {
        self.root.path().join("outside.txt")
    }

    fn path_set(&self) -> PathBuf {
        self.root.path().join("path-set")
    }

    fn exec(&self, enforce: bool, command: &[&str]) -> Output {
        let mut sandbox = Command::new(sandbox_binary());
        sandbox.args(["land", "sandbox", "exec", "--tree"]).arg(self.tree()).current_dir(self.tree()).env_remove("LANG").env_remove("LC_ALL").env_remove("LANGUAGE").env_remove("LOCALE_ARCHIVE");
        if enforce {
            sandbox.arg("--enforce").arg(self.path_set());
        }
        sandbox.arg("--").args(command).env_remove("CARGO_TARGET_DIR").output().unwrap()
    }

    fn cat(&self, enforce: bool, file: &Path) -> Output {
        self.exec(enforce, &["cat", file.to_str().unwrap()])
    }
}

#[test]
fn a_command_reading_a_listed_tree_file_passes() {
    let scene = Scene::new();
    let output = scene.cat(true, &scene.tree().join("listed.txt"));
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"listed\n");
}

#[test]
fn a_command_reading_an_unlisted_tree_file_is_refused() {
    let scene = Scene::new();
    let unlisted = scene.tree().join("unlisted.txt");
    let output = scene.cat(true, &unlisted);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "the fence let the command read {}: {}", unlisted.display(), String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("{}: Permission denied", unlisted.display())), "the read was not refused by the kernel fence: {stderr}");
}

#[test]
fn a_command_reading_a_file_outside_every_root_is_refused() {
    let scene = Scene::new();
    let output = scene.cat(true, &scene.outside());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn without_enforce_the_same_reads_pass() {
    let scene = Scene::new();
    for file in [scene.tree().join("listed.txt"), scene.tree().join("unlisted.txt"), scene.outside()] {
        let output = scene.cat(false, &file);
        assert!(output.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn the_command_gets_a_writable_scratch_directory_that_is_gone_after_the_run() {
    let scene = Scene::new();
    let output = scene.exec(true, &["mktemp", "-d"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let scratch = String::from_utf8(output.stdout).unwrap();
    assert!(!Path::new(scratch.trim_end()).exists());
}

#[test]
fn the_exit_status_of_the_command_is_the_exit_status_of_the_run() {
    let scene = Scene::new();
    assert_eq!(scene.exec(false, &["sh", "-c", "exit 7"]).status.code(), Some(7));
    assert_eq!(scene.exec(false, &["sh", "-c", "kill -TERM $$"]).status.code(), Some(143));
}

fn sandbox_binary() -> std::path::PathBuf {
    option_env!("CARGO_BIN_EXE_north-land-sandbox").map_or_else(
        || std::path::absolute(std::env::var_os("CARGO_BIN_EXE_north-land-sandbox").expect("CARGO_BIN_EXE_north-land-sandbox is set at compile or run time")).expect("the working directory resolves"),
        std::path::PathBuf::from,
    )
}
