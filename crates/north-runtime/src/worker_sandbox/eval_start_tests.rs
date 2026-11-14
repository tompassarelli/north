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
fn only_a_registered_script_on_this_runtime_whose_holds_are_free_starts_and_each_start_is_recorded() {
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

    let told = serve(start("b/start-block", "blk-1", &["5:5", "--max-load", "30"]), "release-evals-lead", &host, &units).unwrap();
    assert!(told.starts_with("eval-start-blk-1 started from b/start-block") && told.contains(&home.join("eval-data/starts/blk-1.log").display().to_string()), "{told}");
    assert_eq!(units.started.borrow()[0].1, [evals.join("b/start-block").display().to_string(), "5:5".to_owned(), "--max-load".to_owned(), "30".to_owned()]);
    let record = recorded(&Label::parse("blk-1").unwrap(), &host).unwrap();
    assert_eq!((record["by"].as_str(), record["unit"].as_str(), record["holds"][0].as_str()), (Some("release-evals-lead"), Some("eval-start-blk-1"), Some("cpx62b")));

    units.active.borrow_mut().push("eval-start-blk-1".to_owned());
    let refused = serve(start("b/other-block", "blk-2", &[]), "release-evals-lead", &host, &units).unwrap_err();
    assert!(refused.contains("holds cpx62b"), "{refused}");
    units.active.borrow_mut().clear();
    serve(start("b/other-block", "blk-2", &[]), "release-evals-lead", &host, &units).unwrap();

    std::fs::write(evals.join("b/start-block"), "#!/bin/sh\n# eval-runtime: any\n# eval-holds: none\nrm -rf /\n").unwrap();
    std::os::unix::fs::symlink(evals.join("b/other-block"), evals.join("b/linked")).unwrap();
    for (name, why) in [("b/start-block", "differs from its checkout's head"), ("b/unregistered", "not registered"), ("b/old-runtime", "measures the runtime 1111111111"), ("b/no-header", "names no runtime"), ("b/linked", "through a link"), ("a/absent", "no registered evaluation directory")] {
        let refused = serve(start(name, "blk-3", &[]), "release-evals-lead", &host, &units).unwrap_err();
        assert!(refused.contains(why), "{name}: {refused}");
    }
    assert_eq!(units.started.borrow().len(), 2, "a refused script was started");
    assert_eq!(records(&host).len(), 2);
    for (script, arguments) in [("../b/start-block", &[][..]), ("/etc/passwd", &[]), ("b/.hidden", &[]), ("b/start-block", &["$(id)"]), ("b/start-block", &["a b"])] {
        assert!(Start::parse(script, "blk-4", arguments).is_none(), "{script} {arguments:?}");
    }
}
