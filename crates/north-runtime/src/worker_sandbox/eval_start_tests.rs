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

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
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

#[test]
fn no_script_starts_while_the_registered_list_is_empty_even_a_tracked_one_on_this_runtime() {
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
    script(&evals, "b/unregistered", "# eval-runtime: any\n# eval-holds: none");
    let units = Recorded::default();

    let refused = serve(start("b/start-block", "blk-1", &[]), "release-evals-lead", &host, &units).unwrap_err();
    assert!(refused.contains("no evaluation directory is registered"), "{refused}");
    assert!(units.started.borrow().is_empty(), "a script started while eval_start_directories is empty");
    for (script, arguments) in [("../b/start-block", &[][..]), ("/etc/passwd", &[]), ("b/.hidden", &[]), ("b/start-block", &["$(id)"]), ("b/start-block", &["a b"])] {
        assert!(Start::parse(script, "blk-4", arguments).is_none(), "{script} {arguments:?}");
    }
}
