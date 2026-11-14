use std::cell::RefCell;
use std::path::Path;

use super::*;

const SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";

type Ran = (Option<String>, String, Vec<String>);

#[derive(Default)]
struct Recorded {
    ran: RefCell<Vec<Ran>>,
}

impl Reach for Recorded {
    fn destination(&self, host: &str) -> Result<Option<String>, String> {
        Ok(Some(format!("tom@{host}")))
    }

    fn run(&self, destination: Option<&str>, program: &str, arguments: &[String]) -> Result<String, String> {
        self.ran.borrow_mut().push((destination.map(str::to_owned), program.to_owned(), arguments.to_vec()));
        Ok(String::new())
    }
}

fn host(root: &Path, role: &str) -> Host {
    let (home, fleet) = (root.join("home"), root.join("fleet"));
    std::fs::create_dir_all(fleet.join("lead")).unwrap();
    std::fs::write(fleet.join("lead").join("host.scope"), "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n").unwrap();
    std::fs::write(fleet.join("lead").join("session.json"), serde_json::json!({ "name": "lead", "role": role, "cwd": home }).to_string()).unwrap();
    Host { fleet, code: vec![home.join("code")], home, environment: Vec::new(), cgroups: PathBuf::from("/sys/fs/cgroup") }
}

fn ask(words: &[&str]) -> Result<Ask, String> {
    Ask::asked(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn a_confined_lead_refreshes_a_listed_host_copy_and_a_path_outside_the_list_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let lead = host(&scratch.path().canonicalize().unwrap(), "lead");
    let listed = Listed::harness();
    let copy = &listed.copies[0];
    assert_eq!((copy.name.as_str(), copy.host.as_str()), ("eval-a", "cpx62"));
    std::fs::create_dir_all(lead.home.join(&copy.from)).unwrap();
    let reach = Recorded::default();
    let peer = Peer::from(Some(SCOPE));

    let told = serve(ask(&["host-op", "sync", "eval-a"]).unwrap(), &peer, &lead, &listed, &reach).unwrap();
    assert!(told.starts_with("eval-a refreshed on cpx62"), "{told}");
    {
        let ran = reach.ran.borrow();
        let (destination, program, arguments) = &ran[0];
        assert_eq!((destination.as_deref(), program.as_str()), (None, "rsync"));
        assert_eq!(arguments[arguments.len() - 2..], [format!("{}/", lead.home.join(copy.from.trim_end_matches('/')).display()), format!("tom@cpx62:{}/", copy.to.trim_end_matches('/'))]);
    }

    for words in [&["host-op", "sync", "/home/tom/.ssh"][..], &["host-op", "sync", "../eval-a"], &["host-op", "sync", "eval-a", "/tmp/x"], &["host-op", "sync", "--rsync-path=sh"]] {
        assert!(ask(words).is_err(), "{words:?}");
    }
    for words in [&["host-op", "sync", "home"][..], &["host-op", "restart", "cpx62", "sshd.service"], &["host-op", "state", "cpx62", "eval-a-x.service"]] {
        assert!(serve(ask(words).unwrap(), &peer, &lead, &listed, &reach).is_err(), "{words:?}");
    }
    assert_eq!(reach.ran.borrow().len(), 1, "a request outside the allow-list ran something");

    let worker = host(&scratch.path().canonicalize().unwrap(), "worker");
    assert!(serve(ask(&["host-op", "sync", "eval-a"]).unwrap(), &peer, &worker, &listed, &reach).unwrap_err().contains("lead session"));
    assert_eq!(reach.ran.borrow().len(), 1, "a worker's request ran something");
}
