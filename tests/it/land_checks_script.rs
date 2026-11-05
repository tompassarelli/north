use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
}

struct Run {
    passed: bool,
    output: String,
}

impl Run {
    fn selected(&self, test: &str) -> bool {
        self.output.contains(&format!("land check: {test}\n"))
    }
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

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        for dir in ["bin", "lib", "harness/hooks", "harness/lib", "harness/scripts"] {
            fs::create_dir_all(repo.join(dir)).unwrap();
        }
        git(&repo, &["init", "-q", "-b", "main"]);
        Fixture { root, repo }
    }

    fn write(&self, path: &str, body: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn commit(&self, files: &[(&str, &str)]) -> String {
        for (path, body) in files {
            self.write(path, body);
        }
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-qm", "change"]);
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn change(&self, files: &[(&str, &str)]) -> String {
        let base = git(&self.repo, &["rev-parse", "HEAD"]);
        self.commit(files);
        base
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .current_dir(&self.repo)
            .arg("land-tests")
            .args(arguments)
            .env("HOME", self.root.path().join("home"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env_remove("SAFE_PUSH_LAND_BASE")
            .env_remove("SAFE_PUSH_LAND_HEAD")
            .env_remove("NORTH_PACE_BIN")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, mut command: Command) -> Run {
        let output = command.output().unwrap();
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run { passed: output.status.success(), output: text }
    }

    fn select(&self, base: &str, only: Option<&str>) -> Run {
        let mut arguments = vec!["--select", "changed"];
        arguments.extend(only.map(|only| ["--only", only]).into_iter().flatten());
        let mut command = self.command(&arguments);
        command.env("SAFE_PUSH_LAND_BASE", base).env("SAFE_PUSH_LAND_HEAD", "HEAD");
        self.run(command)
    }
}

fn importers() -> Fixture {
    let fixture = Fixture::new();
    fixture.commit(&[
        ("harness/lib/events.py", "value = 1\n"),
        ("harness/lib/landing-health.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nimport events\nprint(\"importer ran\")\n"),
        ("harness/lib/bridge.py", "from events import value\n"),
        ("harness/lib/from.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nfrom events import value\n"),
        ("harness/lib/multi.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nimport os, events as renamed\n"),
        ("harness/lib/indented.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\ndef check():\n    from events import value\n    import events\ncheck()\n"),
        ("harness/lib/inline.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nif True: import events\n"),
        ("harness/hooks/embedded.test.sh", "python3 - <<\"PY\"\nimport sys\nsys.path.insert(0, \"harness/lib\")\nfrom events import value\nPY\n"),
        ("harness/lib/transitive.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nimport bridge\nprint(\"transitive importer ran\")\n"),
        ("harness/lib/unrelated.test.py", "import events_extra\nraise RuntimeError(\"unrelated test selected\")\n"),
    ]);
    fixture
}

#[test]
fn indented_and_inline_importers_selected_without_ripgrep_and_git_errors_fail_closed() {
    let fixture = importers();
    let base = fixture.change(&[("harness/lib/events.py", "value = 2\n")]);
    let run = fixture.select(&base, None);
    assert!(run.selected("harness/lib/indented.test.py") && run.selected("harness/lib/inline.test.py"), "{}", run.output);
    let fake = fixture.root.path().join("fake-bin");
    fs::create_dir_all(&fake).unwrap();
    let called = fixture.root.path().join("rg-called");
    north_core::foundation::shim::write_script(&fake.join("rg"), &format!("touch {}\nexit 2\n", called.display()));
    let path = std::env::join_paths(std::iter::once(fake).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let mut command = fixture.command(&["--select", "changed"]);
    command.env("PATH", &path).env("SAFE_PUSH_LAND_BASE", &base).env("SAFE_PUSH_LAND_HEAD", "HEAD");
    let run = fixture.run(command);
    assert!(run.selected("harness/lib/indented.test.py") && run.selected("harness/lib/inline.test.py"), "selection with a broken rg on PATH: {}", run.output);
    assert!(!called.exists(), "land-tests --select ran rg, which a CI runner may not have");
    let run = fixture.select("missing-ref", None);
    assert!(!run.passed && run.output.contains("land check: git diff failed"), "{}", run.output);
}

