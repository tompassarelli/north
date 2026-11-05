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
    fn count(&self, line: &str) -> usize {
        self.output.lines().filter(|candidate| *candidate == line).count()
    }

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

fn on_path(program: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join(program)).find(|path| path.is_file()).unwrap_or_else(|| panic!("{program} is not on PATH"))
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
fn land_checks_run_direct_and_transitive_importers_and_path_callers_once_excluding_unrelated_tests() {
    let fixture = importers();
    let base = fixture.change(&[("harness/lib/events.py", "value = 2\n")]);
    let run = fixture.select(&base, None);
    for test in ["landing-health", "from", "multi", "indented", "inline", "transitive"] {
        assert_eq!(run.count(&format!("land check: harness/lib/{test}.test.py")), 1, "{}", run.output);
    }
    assert!(run.passed && run.output.contains("importer ran") && run.output.contains("transitive importer ran") && !run.output.contains("unrelated.test.py"), "{}", run.output);
    assert_eq!(run.count("land check: harness/hooks/embedded.test.sh"), 1, "{}", run.output);
    fixture.commit(&[
        ("bin/tool", "printf \"tool ran\\n\"\n"),
        ("lib/invoker.test.sh", "bash bin/tool\n"),
        ("bin/tool.test.sh", "bash bin/tool\n"),
        ("harness/scripts/task.sh", "printf \"script ran\\n\"\n"),
        ("harness/hooks/caller.test.sh", "bash harness/scripts/task.sh\n"),
    ]);
    let base = fixture.change(&[("bin/tool", "printf \"tool changed\\n\"\n"), ("harness/scripts/task.sh", "printf \"script changed\\n\"\n")]);
    let run = fixture.select(&base, None);
    for test in ["bin/tool.test.sh", "lib/invoker.test.sh", "harness/hooks/caller.test.sh"] {
        assert_eq!(run.count(&format!("land check: {test}")), 1, "{}", run.output);
    }
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

#[test]
fn path_selection_excludes_longer_filenames_with_a_shared_prefix() {
    let fixture = Fixture::new();
    fixture.commit(&[
        ("bin/widget", "value = 1\n"),
        ("bin/widget.test.sh", "printf \"widget test ran\\n\"\n"),
        ("lib/long-name.test.py", "raise SystemExit(\"longer filename selected\")\nlauncher = \"bin/widget-server\"\n"),
    ]);
    let base = fixture.change(&[("bin/widget", "value = 2\n")]);
    let run = fixture.select(&base, None);
    assert!(run.selected("bin/widget.test.sh") && !run.selected("lib/long-name.test.py"), "{}", run.output);
}

fn hooks() -> Fixture {
    let fixture = Fixture::new();
    fixture.commit(&[
        ("harness/hooks/gate.sh", "python3 \"$(dirname \"$0\")/lib/decide.py\"\n"),
        ("harness/hooks/lib/decide.py", "import usage\n"),
        ("harness/lib/usage.py", "value = 1\n"),
        ("harness/hooks/gate.test.sh", "printf \"gate test ran\\n\"\n"),
        ("harness/lib/pace_client.py", "subprocess.run([os.environ.get(\"NORTH_PACE_BIN\", \"north\"), \"pace\"])\n"),
        ("harness/hooks/lib/paced.py", "import pace_client\n"),
        ("harness/hooks/paced.sh", "python3 \"$(dirname \"$0\")/lib/paced.py\"\n"),
        ("harness/hooks/paced.test.sh", "printf \"paced test ran\\n\"\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("harness/hooks/direct.test.sh", "north=\"$(dirname \"$0\")/../../target/debug/north\"; printf \"direct test ran\\n\"\n"),
        ("harness/registry.toml", "consumers = [\"src/main.rs\"]\n"),
        ("harness/lib/registry.test.py", "print(open(\"harness/registry.toml\").read())\n"),
        ("harness/lib/ledger_client.py", "binary_variable = \"NORTH_BIN\"\n"),
        ("harness/lib/ledger.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nimport ledger_client\nprint(\"ledger test ran\")\n"),
    ]);
    fixture
}

#[test]
fn a_hook_test_runs_when_a_library_its_hook_imports_or_the_north_binary_it_calls_changes_and_a_red_one_fails_the_check() {
    let fixture = hooks();
    let base = fixture.change(&[("harness/lib/usage.py", "value = 2\n")]);
    let run = fixture.select(&base, None);
    assert!(run.selected("harness/hooks/gate.test.sh") && !run.selected("harness/hooks/paced.test.sh"), "{}", run.output);
    let base = fixture.change(&[("src/main.rs", "fn main() { }\n")]);
    let run = fixture.select(&base, None);
    for test in ["harness/hooks/paced.test.sh", "harness/hooks/direct.test.sh", "harness/lib/ledger.test.py", "harness/lib/registry.test.py"] {
        assert!(run.selected(test), "{test}: {}", run.output);
    }
    assert!(!run.selected("harness/hooks/gate.test.sh"), "{}", run.output);
    let base = fixture.change(&[("harness/hooks/gate.test.sh", "exit 1\n"), ("harness/lib/usage.py", "value = 3\n")]);
    assert!(!fixture.select(&base, None).passed);
}

#[test]
fn a_change_under_crates_north_core_src_threads_or_ledger_selects_every_test_that_runs_the_north_binary() {
    let fixture = hooks();
    for dir in ["threads/calibration", "ledger"] {
        let base = fixture.change(&[(&format!("crates/north-core/src/{dir}/x.rs"), "fn f() {}\n")]);
        let run = fixture.select(&base, None);
        assert!(run.selected("harness/hooks/direct.test.sh") && run.selected("harness/lib/ledger.test.py"), "{dir}: {}", run.output);
    }
}

#[test]
fn any_crates_change_crate_manifest_or_build_rs_selects_the_tests_that_run_the_north_binary_and_not_unrelated_ones() {
    let fixture = Fixture::new();
    fixture.commit(&[
        ("harness/hooks/direct.test.sh", "north=\"$(dirname \"$0\")/../../target/debug/north\"; printf \"direct test ran\\n\"\n"),
        ("harness/lib/ledger_client.py", "binary_variable = \"NORTH_BIN\"\n"),
        ("harness/lib/ledger.test.py", "import sys\nsys.path.insert(0, \"harness/lib\")\nimport ledger_client\nprint(\"ledger test ran\")\n"),
        ("harness/hooks/unrelated.test.sh", "printf \"unrelated test ran\\n\"\n"),
        ("crates/north-core/src/lib.rs", "fn main() {}\n"),
    ]);
    for change in ["crates/north-landing/src/x.rs", "crates/north-base/src/y.rs", "crates/north-core/src/lib.rs", "crates/north-core/Cargo.toml", "build.rs"] {
        let base = fixture.change(&[(change, &format!("changed {change}\n"))]);
        let run = fixture.select(&base, Some("portable"));
        let running = run.output.lines().find(|line| line.starts_with("land check: running ")).unwrap_or_default();
        assert!(
            run.selected("harness/hooks/direct.test.sh")
                && run.selected("harness/lib/ledger.test.py")
                && !run.output.contains("unrelated.test.sh")
                && running.contains(" land-tests ")
                && running.contains("harness/hooks/direct.test.sh")
                && run.output.contains("land check: started harness/hooks/direct.test.sh"),
            "{change}: {}",
            run.output
        );
    }
}

#[test]
fn a_listed_machine_bound_test_file_that_does_not_exist_fails_the_run_and_is_named() {
    let fixture = Fixture::new();
    fixture.commit(&[("bin/present.test.sh", "printf \"present\\n\"\n"), ("harness/machine-bound-tests", "# listed\nbin/present.test.sh\nharness/hooks/gone.test.sh\n")]);
    for only in ["portable", "machine-bound"] {
        let run = fixture.select("HEAD", Some(only));
        assert!(!run.passed && run.output.contains("harness/hooks/gone.test.sh") && !run.output.contains("bin/present.test.sh"), "{only}: {}", run.output);
    }
}

#[test]
fn portable_all_selects_every_portable_test_under_the_roots_without_a_diff() {
    let fixture = Fixture::new();
    for (path, body) in [
        ("bin/a.test.sh", "printf \"all-a ran\\n\"\n"),
        ("harness/hooks/b.test.py", "print(\"all-b ran\")\n"),
        ("lib/c.test.sh", "printf \"all-c ran\\n\"\n"),
        ("harness/scripts/d.test.sh", "printf \"all-d ran\\n\"\n"),
        ("crates/x/e.test.sh", "printf \"all-e ran\\n\"\n"),
    ] {
        fixture.write(path, body);
    }
    let run = fixture.run(fixture.command(&["--select", "all", "--only", "portable"]));
    for test in ["bin/a.test.sh", "harness/hooks/b.test.py", "lib/c.test.sh", "harness/scripts/d.test.sh"] {
        assert_eq!(run.count(&format!("land check: {test}")), 1, "{test}: {}", run.output);
    }
    assert!(run.passed && !run.output.contains("crates/x/e.test.sh"), "{}", run.output);
}

#[test]
fn a_change_under_crates_north_core_src_threads_calibration_selects_bin_threads_test_sh_in_portable_mode_and_bin_run_bounded_test_sh_in_machine_bound_mode_both_in_the_all_mode() {
    let fixture = Fixture::new();
    fixture.commit(&[
        ("bin/threads.test.sh", "north=\"$NORTH_BIN\"; printf \"threads test ran\\n\"\n"),
        ("bin/run-bounded.test.sh", "north=\"$NORTH_BIN\"; printf \"bounded test ran\\n\"\n"),
        ("harness/machine-bound-tests", "# listed\nbin/run-bounded.test.sh\n"),
    ]);
    let base = fixture.change(&[("crates/north-core/src/threads/calibration/x.rs", "fn g() {}\n")]);
    for (only, selected, absent) in [("portable", "bin/threads.test.sh", "bin/run-bounded.test.sh"), ("machine-bound", "bin/run-bounded.test.sh", "bin/threads.test.sh")] {
        let run = fixture.select(&base, Some(only));
        assert!(run.selected(selected) && !run.selected(absent), "{only}: {}", run.output);
    }
    let run = fixture.select(&base, None);
    assert!(run.selected("bin/threads.test.sh") && run.selected("bin/run-bounded.test.sh"), "{}", run.output);
}

#[test]
fn both_land_tests_checks_in_safe_push_run_for_a_change_under_crates() {
    let safe_push = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".safe-push")).unwrap();
    for only in ["portable", "machine-bound"] {
        let line = safe_push
            .lines()
            .find(|line| line.trim_start().starts_with("after = ") && line.ends_with(&format!("land-tests --select changed --only {only}")))
            .unwrap_or_else(|| panic!("no .safe-push after line runs the {only} land tests"));
        let filter = line.trim_start().trim_start_matches("after = ").split(" :: ").next().unwrap();
        assert!(filter.split(',').any(|path| path == "crates/"), "{line}");
    }
}

#[test]
fn land_checks_keep_resolving_git_and_their_tools_after_a_profile_directory_on_path_is_removed_mid_run() {
    let fixture = Fixture::new();
    let git_dir = on_path("git").parent().unwrap().to_owned();
    let profile = fixture.root.path().join("profile-bin");
    std::os::unix::fs::symlink(&git_dir, &profile).unwrap();
    fixture.commit(&[
        ("bin/vanish", "value = 1\n"),
        ("bin/vanish.test.sh", "set -e\nrm -f \"$PROFILE_LINK\"\ngit --version >/dev/null\nprintf \"tools survived the switch\\n\"\n"),
    ]);
    let base = fixture.change(&[("bin/vanish", "value = 2\n")]);
    let inherited = std::env::var_os("PATH").unwrap();
    let trimmed = std::env::split_paths(&inherited).filter(|dir| *dir != git_dir);
    let path = std::env::join_paths(std::iter::once(profile.clone()).chain(trimmed)).unwrap();
    let mut command = fixture.command(&["--select", "changed"]);
    command.env("PATH", path).env("PROFILE_LINK", &profile).env("SAFE_PUSH_LAND_BASE", &base).env("SAFE_PUSH_LAND_HEAD", "HEAD");
    let run = fixture.run(command);
    assert!(run.passed && run.output.contains("tools survived the switch"), "{}", run.output);
}
