use super::*;
use std::cell::{Cell, RefCell};

const RULES: &str = "title = \"attest fixture\"\n[[rules]]\nid = \"fake-private-term\"\nregex = '''zebra-quokka-7731'''\n";

#[derive(Default)]
struct Forge {
    remote: PathBuf,
    tips: Vec<BranchTip>,
    states: Vec<(String, String, String)>,
    refuse_dispatch: Cell<bool>,
    post_answer: RefCell<Option<PostError>>,
    posted: RefCell<Vec<(String, String, String, String)>>,
    dispatched: RefCell<Vec<(String, String, String)>>,
}

impl CommitStatuses for Forge {
    fn matching_branches(&self, _repo: &str, prefix: &str) -> Result<Vec<BranchTip>, String> {
        Ok(self.tips.iter().filter(|tip| tip.branch.starts_with(prefix)).cloned().collect())
    }

    fn status_state(&self, _repo: &str, sha: &str, context: &str) -> Result<Option<String>, String> {
        Ok(self.states.iter().find(|(held, named, _)| held == sha && named == context).map(|(_, _, state)| state.clone()))
    }

    fn post_status(&self, repo: &str, sha: &str, context: &str, state: &str, description: &str) -> Result<(), PostError> {
        assert_eq!(repo, "owner/public");
        if let Some(error) = self.post_answer.borrow_mut().take() {
            return Err(error);
        }
        self.posted.borrow_mut().push((sha.into(), context.into(), state.into(), description.into()));
        Ok(())
    }

    fn dispatch_workflow(&self, repo: &str, workflow: &str, branch: &str) -> Result<(), String> {
        if self.refuse_dispatch.get() {
            return Err("dispatch refused".into());
        }
        self.dispatched.borrow_mut().push((repo.into(), workflow.into(), branch.into()));
        Ok(())
    }

    fn clone_url(&self, _repo: &str) -> String {
        self.remote.to_string_lossy().into_owned()
    }
}

struct Fixture {
    scratch: tempfile::TempDir,
    work: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let work = scratch.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(scratch.path(), &["init", "-q", "--bare", "remote.git"]).unwrap();
        for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "fixture"], &["config", "user.email", "fixture@example.invalid"]] {
            git(&work, args).unwrap();
        }
        std::fs::write(scratch.path().join("rules.toml"), RULES).unwrap();
        let fixture = Fixture { scratch, work };
        fixture.commit("README.md", "public repository\n", "base");
        fixture.push("main");
        fixture
    }

    fn commit(&self, path: &str, text: &str, message: &str) -> String {
        std::fs::write(self.work.join(path), text).unwrap();
        git(&self.work, &["add", "-A"]).unwrap();
        git(&self.work, &["-c", "commit.gpgsign=false", "commit", "-qm", message]).unwrap();
        git(&self.work, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
    }

    fn push(&self, branch: &str) {
        let remote = self.scratch.path().join("remote.git");
        git(&self.work, &["push", "-q", &remote.to_string_lossy(), &format!("HEAD:refs/heads/{branch}")]).unwrap();
    }

    fn tip(&self, branch: &str, text: &str) -> BranchTip {
        git(&self.work, &["checkout", "-q", "-B", branch, "main"]).unwrap();
        let sha = self.commit("change.txt", text, "change");
        self.push(branch);
        git(&self.work, &["checkout", "-q", "main"]).unwrap();
        BranchTip { branch: branch.into(), sha }
    }

    fn forge(&self, tips: Vec<BranchTip>) -> Forge {
        Forge { remote: self.scratch.path().join("remote.git"), tips, ..Forge::default() }
    }

    fn attest(&self, forge: &Forge, config: &str, branch: Option<&str>) -> u8 {
        let request = Request { south: PathBuf::new(), repos: vec!["owner/public".into()], branch: branch.map(str::to_owned) };
        attest_all(forge, &self.scratch.path().join(config), &self.scratch.path().join("cache"), &request)
    }
}

#[test]
fn denylist_attest_finding_posts_a_failure_naming_no_rule_or_value_and_no_dispatch() {
    let fixture = Fixture::new();
    let dirty = fixture.tip("claude/dirty-work", "owner: zebra-quokka-7731\n");
    let forge = fixture.forge(vec![dirty.clone()]);
    assert_eq!(fixture.attest(&forge, "rules.toml", Some("claude/dirty-work")), 1);
    let posted = forge.posted.borrow();
    assert_eq!(*posted, vec![(dirty.sha, "denylist".into(), "failure".into(), "deny-list refused: reword and push again".into())]);
    assert!(!posted[0].3.contains("fake-private-term") && !posted[0].3.contains("zebra-quokka-7731"));
    assert!(forge.dispatched.borrow().is_empty());
}

#[test]
fn denylist_attest_scan_or_fetch_error_posts_nothing_and_exits_2() {
    let fixture = Fixture::new();
    let tip = fixture.tip("claude/some-work", "ordinary words\n");
    let forge = fixture.forge(vec![tip.clone()]);
    assert_eq!(fixture.attest(&forge, "absent-rules.toml", None), 2);
    let moved = BranchTip { branch: tip.branch, sha: "0123456789abcdef0123456789abcdef01234567".into() };
    let forge = fixture.forge(vec![moved]);
    assert_eq!(fixture.attest(&forge, "rules.toml", None), 2);
    assert!(forge.posted.borrow().is_empty() && forge.dispatched.borrow().is_empty());
}

