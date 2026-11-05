use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NORTH_STUB: &str = "case \"$1\" in\n  land-cache) exit 1 ;;\nesac\nexec \"$REAL_NORTH\" \"$@\"\n";
const CARGO_STUB: &str = "[ \"$1\" != metadata ] || exec \"$REAL_CARGO\" \"$@\"\nprintf 'cargo %s\\n' \"$*\" >>\"$STUB_TRACE\"\ncase \"$1\" in clippy) printf 'error: redundant clone\\n  --> crates/a/src/lib.rs:1:1\\n' >&2; exit \"${STUB_LINT_STATUS:-0}\" ;; esac\n";
const CLIPPY: &str = "cargo clippy --all-targets ${SAFE_PUSH_LAND_PACKAGES:---workspace} --locked --offline -- -D warnings";
const AFFECTED: &str = "cargo clippy --all-targets -p a -p b --locked --offline -- -D warnings";
const WHOLE: &str = "cargo clippy --all-targets --workspace --locked --offline -- -D warnings";
const WORKSPACE: &str = "[workspace]\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\"]\nresolver = \"2\"\n";
const LATER: &str = "printf 'later\\n' >>\"$STUB_TRACE\"";

struct Lane {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
    head: String,
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(repo: &Path, path: &str, body: &str) {
    let file = repo.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, body).unwrap();
}

fn crate_manifest(name: &str, dependency: Option<&str>) -> String {
    let dependencies = dependency.map(|dependency| format!("\n[dependencies]\n{dependency} = {{ path = \"../{dependency}\" }}\n")).unwrap_or_default();
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{dependencies}")
}

fn real(tool: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|directory| directory.join(tool)).find(|path| path.is_file()).unwrap()
}

impl Lane {
    fn changing(changed: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        write(&repo, "Cargo.toml", WORKSPACE);
        for (name, dependency) in [("a", None), ("b", Some("a")), ("c", None)] {
            write(&repo, &format!("crates/{name}/Cargo.toml"), &crate_manifest(name, dependency));
            write(&repo, &format!("crates/{name}/src/lib.rs"), "\n");
        }
        write(&repo, "docs/notes.md", "base\n");
        for check in [format!("** :: {LATER}"), format!("@rust-input,crates/,Cargo.toml,Cargo.lock :: {CLIPPY}")] {
            git(&repo, &["config", "--file", ".safe-push", "--add", "land.check", &check]);
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        for path in changed {
            let body = if *path == "Cargo.toml" { format!("{WORKSPACE}\n") } else { "changed\n".to_owned() };
            write(&repo, path, &body);
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "lane"]);
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let stubs = root.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        shim::write_executable(&stubs.join("north"), format!("#!/usr/bin/env bash\n{NORTH_STUB}"));
        shim::write_executable(&stubs.join("cargo"), format!("#!/usr/bin/env bash\n{CARGO_STUB}"));
        shim::write_executable(&stubs.join("safe-push"), "#!/usr/bin/env bash\nexit 0\n");
        for directory in ["state", "runtime"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        fs::write(root.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
        Self { root, repo, base, head }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn checks(&self, lint_status: i32) -> Output {
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", &self.base, "--head", &self.head, "--safe-push"])
            .arg(self.path("stubs/safe-push"))
            .arg("--outcome")
            .arg(self.path("outcome"))
            .env("PATH", path)
            .env("NORTH_LAND_NORTH", self.path("stubs/north"))
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("REAL_CARGO", real("cargo"))
            .env("STUB_TRACE", self.path("trace"))
            .env("STUB_LINT_STATUS", lint_status.to_string())
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env("AGENT_CAPACITY_LOADAVG", self.path("loadavg"))
            .env("AGENT_CAPACITY_CORES", "4")
            .env_remove("AGENT_CAPACITY_LEASE")
            .env_remove("SAFE_PUSH_LAND_CAPACITY")
            .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
            .env_remove("SAFE_PUSH_LAND_RUNNER")
            .env_remove("SAFE_PUSH_LAND_CHECK_TIMEOUT")
            .env_remove("NORTH_LAND_ROUND")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn trace(&self) -> Vec<String> {
        fs::read_to_string(self.path("trace")).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn a_lane_changing_one_crate_runs_clippy_on_it_and_its_dependents_before_every_other_check() {
    let lane = Lane::changing(&["crates/a/src/lib.rs", "docs/notes.md"]);
    let output = lane.checks(0);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(text(&output).contains(&format!("safe-push: land check order: 1. clippy on the affected crates (-p a -p b): {CLIPPY}; then 1 land check(s): {LATER}")), "{}", text(&output));
    assert_eq!(lane.trace(), [AFFECTED, "later"]);
}

#[test]
fn a_lint_failure_in_the_affected_crates_ends_the_check_at_once_with_the_clippy_output() {
    let lane = Lane::changing(&["crates/a/src/lib.rs"]);
    let output = lane.checks(101);
    assert_eq!(output.status.code(), Some(101), "{}", text(&output));
    let printed = text(&output);
    assert!(printed.contains("error: redundant clone"), "the clippy output was not shown:\n{printed}");
    assert!(printed.contains("safe-push: clippy on the affected crates exited 101, so the other 1 land check(s) did not run; NOT pushed."), "{printed}");
    assert_eq!(lane.trace(), [AFFECTED], "a later check ran after the lint failure");
}

#[test]
fn a_lane_changing_the_root_manifest_or_lockfile_runs_clippy_on_the_whole_workspace_first() {
    for changed in ["Cargo.lock", "Cargo.toml"] {
        let lane = Lane::changing(&[changed]);
        let output = lane.checks(0);
        assert_eq!(output.status.code(), Some(0), "{changed}: {}", text(&output));
        assert_eq!(lane.trace(), [WHOLE, "later"], "{changed}");
        assert!(text(&output).contains("1. clippy on the affected crates (--workspace)"), "{}", text(&output));
    }
}

#[test]
fn a_lane_changing_no_rust_file_skips_the_clippy_first_step() {
    let lane = Lane::changing(&["docs/notes.md"]);
    let output = lane.checks(101);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(!text(&output).contains("land check order"), "{}", text(&output));
    assert_eq!(lane.trace(), ["later"]);
}
