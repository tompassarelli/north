use north_base::test_inputs::git::git_in as git;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::worker_sandbox::eval_units::Shown;

const EVALS: &str = "code/south/main/agents/decisions/harness/overhaul-2026-11-11/evals";

#[derive(Default)]
struct Recorded {
    started: RefCell<Vec<(String, Vec<String>)>>,
    active: RefCell<Vec<String>>,
}

impl Units for Recorded {
    fn start(&self, unit: &Unit) -> Result<(), String> {
        self.started.borrow_mut().push((unit.name.clone(), unit.command.clone()));
        Ok(())
    }

    fn show(&self, name: &str) -> Result<Shown, String> {
        let state = if self.active.borrow().iter().any(|active| active == name) { "active" } else { "inactive" };
        Ok(Shown { id: format!("{name}.service"), description: String::new(), state: state.to_owned(), result: "success".to_owned() })
    }

    fn stop(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

fn script(evals: &Path, name: &str, header: &str) {
    let path = evals.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("#!/bin/sh\n{header}\necho ran\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn start(script: &str, label: &str, arguments: &[&str]) -> Start {
    Start::parse(script, label, arguments).unwrap()
}

fn landed() -> (tempfile::TempDir, Host, PathBuf, PathBuf) {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let host = Host { fleet: home.join("fleet"), code: vec![home.join("code")], home: home.clone(), environment: vec![("PATH".to_owned(), std::env::var("PATH").unwrap())], cgroups: PathBuf::from("/sys/fs/cgroup") };
    std::fs::create_dir_all(home.join(".local/state/north/current")).unwrap();
    std::fs::write(home.join(RUNTIME_PIN), "4903316be5aa 00\n").unwrap();
    let checkout = home.join("code/south/main");
    let evals = home.join(EVALS);
    script(&evals, "b/start-block", "# eval-runtime: 4903316be5\n# eval-holds: cpx62b");
    script(&evals, "b/other-block", "# eval-runtime: 4903316be5\n# eval-holds: cpx62b");
    script(&evals, "b/old-runtime", "# eval-runtime: 1111111111\n# eval-holds: none");
    script(&evals, "b/no-header", "");
    git(&checkout, &["init", "-q", "-b", "main"]);
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "registered"]);
    let origin = home.join("origin.git");
    git(&home, &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]);
    git(&checkout, &["remote", "add", "origin", &origin.to_string_lossy()]);
    git(&checkout, &["push", "-q", "origin", "main"]);
    north_trusted_mirror::Mirror::open(&checkout).unwrap();
    (scratch, host, checkout, evals)
}

#[test]
fn only_a_tracked_unchanged_script_under_the_registered_directory_starts_and_an_empty_list_starts_none() {
    let (_scratch, host, _checkout, evals) = landed();
    script(&evals, "b/unregistered", "# eval-runtime: any\n# eval-holds: none");
    let units = Recorded::default();

    let empty = registered(&Script::parse("b/start-block").unwrap(), &[], &host).unwrap_err();
    assert!(empty.contains("no evaluation directory is registered"), "{empty}");
    for (name, refusal) in [("b/unregistered", "the trusted mirror tracks no such file"), ("b/old-runtime", "measures the runtime 1111111111"), ("b/no-header", "names no runtime"), ("c/start-block", "is in no registered evaluation directory")] {
        let refused = serve(start(name, "blk-0", &[]), "evals-lead2", &host, &units).unwrap_err();
        assert!(refused.contains(refusal), "{name}: {refused}");
    }
    assert!(units.started.borrow().is_empty(), "a refused script started");

    let started = serve(start("b/start-block", "blk-1", &[]), "evals-lead2", &host, &units).unwrap();
    assert!(started.contains("blk-1"), "{started}");
    assert_eq!(units.started.borrow().len(), 1);
    units.active.borrow_mut().push(unit_name(&Label::parse("blk-1").unwrap()));
    let shared = serve(start("b/other-block", "blk-2", &[]), "evals-lead2", &host, &units).unwrap_err();
    assert!(shared.contains("holds cpx62b"), "{shared}");
    std::fs::OpenOptions::new().append(true).open(evals.join("b/other-block")).and_then(|mut file| std::io::Write::write_all(&mut file, b"# edited\n")).unwrap();
    let edited = serve(start("b/other-block", "blk-3", &[]), "evals-lead2", &host, &units).unwrap_err();
    assert!(edited.contains("differs from origin/main of the trusted mirror"), "{edited}");
    assert_eq!(units.started.borrow().len(), 1);
    for (script, arguments) in [("../b/start-block", &[][..]), ("/etc/passwd", &[]), ("b/.hidden", &[]), ("b/start-block", &["$(id)"]), ("b/start-block", &["a b"])] {
        assert!(Start::parse(script, "blk-4", arguments).is_none(), "{script} {arguments:?}");
    }
}

#[test]
fn an_eval_script_is_checked_against_the_trusted_mirror() {
    let (_scratch, host, checkout, evals) = landed();
    std::fs::OpenOptions::new().append(true).open(evals.join("b/start-block")).and_then(|mut file| std::io::Write::write_all(&mut file, b"echo a worker's line\n")).unwrap();
    git(&checkout, &["commit", "-q", "-am", "A script the worker changed"]);
    let units = Recorded::default();

    let refused = serve(start("b/start-block", "blk-5", &[]), "evals-lead2", &host, &units).unwrap_err();
    assert!(refused.contains("differs from origin/main of the trusted mirror"), "{refused}");
    assert!(units.started.borrow().is_empty(), "a script changed only in its checkout's head started");
}
