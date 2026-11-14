use std::cell::RefCell;
use std::path::Path;
use std::process::Command;

use super::*;

const LEAD_SESSION: &str = "release-evals-lead";
const SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";
const AGENT: &str = "/run/user/1000/agent.sock";

#[derive(Default)]
struct Recorded {
    started: RefCell<Vec<Unit>>,
    stopped: RefCell<Vec<String>>,
    description: RefCell<String>,
    id: RefCell<String>,
}

impl Units for Recorded {
    fn start(&self, unit: &Unit) -> Result<(), String> {
        self.started.borrow_mut().push(Unit { name: unit.name.clone(), description: unit.description.clone(), environment: unit.environment.clone(), log: unit.log.clone(), command: unit.command.clone(), after: unit.after.clone() });
        Ok(())
    }

    fn show(&self, _: &str) -> Result<Shown, String> {
        Ok(Shown { id: self.id.borrow().clone(), description: self.description.borrow().clone(), state: "active".to_owned(), result: "success".to_owned() })
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        self.stopped.borrow_mut().push(name.to_owned());
        Ok(())
    }
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn host(root: &Path, role: &str, name: &str, agent: bool) -> Host {
    let (home, fleet) = (root.join("home"), root.join("fleet"));
    std::fs::create_dir_all(fleet.join(name)).unwrap();
    std::fs::write(fleet.join(name).join("host.scope"), "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n").unwrap();
    std::fs::write(fleet.join(name).join("session.json"), serde_json::json!({ "name": name, "role": role, "cwd": home }).to_string()).unwrap();
    let agent = agent.then(|| (SSH_AGENT.to_owned(), AGENT.to_owned()));
    Host { fleet, code: vec![home.join("code")], home, environment: vec![("PATH".to_owned(), std::env::var("PATH").unwrap())].into_iter().chain(agent).collect(), cgroups: PathBuf::from("/sys/fs/cgroup") }
}

fn ask(words: &[&str]) -> Ask {
    Ask::asked(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).unwrap().unwrap()
}

pub(in super::super) fn a_listed_lead_starts_reads_and_stops_only_the_finish_proof_unit_and_a_request_outside_the_closed_list_starts_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let lead = host(&root, LEAD, LEAD_SESSION, true);
    let main = lead.home.join("code/north/main");
    std::fs::create_dir_all(main.join("evals/b")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("evals/b/main.rs"), "one").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "one"]);
    let head = git(&main, &["rev-parse", "--short=10", "HEAD"]);
    let base = git(&main, &["rev-parse", "HEAD"]);
    let digest = format!("{:x}", Sha256::digest("runner"));
    let pin = Pin { commit: &base, built: "0123456789", digest: &digest };
    let data = lead.home.join(DATA);
    std::fs::create_dir_all(data.join("bin")).unwrap();
    std::fs::write(data.join("bin").join("eval-b.0123456789"), "runner").unwrap();
    let units = Recorded::default();

    let told = serve(ask(&["eval-unit", "start", "finish-proof", "rel-1114-r10", "2"]), SCOPE, &lead, &units, &pin).unwrap();
    assert!(told.starts_with("eval-b-north-rel-1114-r10 started"), "{told}");
    let started = units.started.borrow();
    let unit = &started[0];
    let runner = data.join("bin").join(format!("eval-b.{head}")).display().to_string();
    let at = |name: &str| format!("{}/{name}", data.display());
    assert_eq!(unit.name, "eval-b-north-rel-1114-r10");
    assert_eq!(unit.command, [&runner, "ladder", "--workload", &at("workload"), "--level", "2", "--count", "1", "--account", "a", "--parent", LEAD_SESSION, "--hosts", "cpx62b", "--work", &at("work"), "--results", &at("results-north-rel-1114-r10.jsonl")].map(str::to_owned));
    assert_eq!(unit.after, [&runner, "queue", "--archive", &at("queue"), "--hosts", "cpx62b"].map(str::to_owned));
    assert_eq!(unit.log, data.join("runs.log"));
    assert_eq!(unit.environment, [("PATH".to_owned(), format!("{0}/.local/bin:{0}/{NIX_PROFILE_BIN}:{SYSTEM_BIN}", lead.home.display())), (SSH_AGENT.to_owned(), AGENT.to_owned())]);

    *units.description.borrow_mut() = unit.description.clone();
    *units.id.borrow_mut() = "eval-b-north-rel-1114-r10.service".to_owned();
    assert_eq!(serve(ask(&["eval-unit", "state", "rel-1114-r10"]), SCOPE, &lead, &units, &pin).unwrap(), "eval-b-north-rel-1114-r10: active, result success");
    serve(ask(&["eval-unit", "stop", "rel-1114-r10"]), SCOPE, &lead, &units, &pin).unwrap();
    assert_eq!(*units.stopped.borrow(), ["eval-b-north-rel-1114-r10"]);
    *units.description.borrow_mut() = "a unit someone else started".to_owned();
    assert!(serve(ask(&["eval-unit", "stop", "other"]), SCOPE, &lead, &units, &pin).is_err());
    *units.description.borrow_mut() = unit.description.clone();
    *units.id.borrow_mut() = "eval-b-north-elsewhere.service".to_owned();
    assert!(serve(ask(&["eval-unit", "stop", "rel-1114-r10"]), SCOPE, &lead, &units, &pin).is_err());
    assert_eq!(units.stopped.borrow().len(), 1, "a unit this request did not start was stopped");

    let other = Pin { commit: &base, built: "0123456789", digest: "x" };
    let refused = serve(ask(&["eval-unit", "start", "finish-proof", "next", "2"]), SCOPE, &lead, &units, &other).unwrap_err();
    assert!(refused.contains("sha256") && units.started.borrow().len() == 1, "{refused}");

    std::fs::write(main.join("evals/b/main.rs"), "two").unwrap();
    git(&main, &["commit", "-q", "-am", "two"]);
    let refused = serve(ask(&["eval-unit", "start", "finish-proof", "next", "2"]), SCOPE, &lead, &units, &pin).unwrap_err();
    assert!(refused.contains("a runner build is a separate matter") && units.started.borrow().len() == 1, "{refused}");

    for words in [
        &["eval-unit", "start", "finish-proof", "../x", "2"][..],
        &["eval-unit", "start", "finish-proof", "Upper", "2"],
        &["eval-unit", "start", "finish-proof", "-x", "2"],
        &["eval-unit", "start", "finish-proof", "x", "0"],
        &["eval-unit", "start", "finish-proof", "x", "61"],
        &["eval-unit", "start", "finish-proof", "x", "+2"],
        &["eval-unit", "start", "ladder", "x", "2"],
        &["eval-unit", "start", "finish-proof", "x", "2", "--hosts", "elsewhere"],
        &["eval-unit", "state"],
    ] {
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        assert!(Ask::asked(&words).unwrap().is_err(), "{words:?}");
    }
    assert!(Ask::asked(&["capacity".to_owned()]).is_none());
}

#[test]
fn only_a_lead_session_the_harness_lists_may_ask_for_an_eval_unit() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let units = Recorded::default();
    for (role, name, cgroup) in [("worker", LEAD_SESSION, SCOPE), ("lead", "another-lead", SCOPE), ("lead", LEAD_SESSION, "0::/user.slice/app.slice/session-3.scope\n")] {
        let host = host(&root, role, name, true);
        let refused = serve(ask(&["eval-unit", "start", "finish-proof", "x", "2"]), cgroup, &host, &units, &Pin { commit: "0000000000", built: "0000000000", digest: "" }).unwrap_err();
        assert!(refused.contains("lead session") || refused.contains("no session's scope"), "{role} {name}: {refused}");
        assert!(serve(ask(&["eval-unit", "stop", "x"]), cgroup, &host, &units, &Pin { commit: "0000000000", built: "0000000000", digest: "" }).is_err());
    }
    assert!(units.started.borrow().is_empty() && units.stopped.borrow().is_empty());
}
