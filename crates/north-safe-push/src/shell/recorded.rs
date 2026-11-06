use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use north_base::foundation::step::{Refused, Step};

use super::Shell;
use super::files::{Lock, Pushed, StateFiles};
use super::git::{Annotation, Git};
use super::github::GitHub;
use super::origin::{Origin, Push, Snapshot};
use super::processes::{Child, Finished, Processes};
use super::settings::Settings;

#[derive(Default)]
pub struct State {
    pub logs: Vec<PathBuf>,
    pub busy: Vec<PathBuf>,
    pub unavailable: Vec<PathBuf>,
    pub locked: Vec<PathBuf>,
    pub events: Vec<String>,
    pub stages: Vec<String>,
    pub detached: Vec<(PathBuf, Vec<String>)>,
}

pub type Shared = Rc<RefCell<State>>;

pub fn shell(state: &Shared, settings: Settings) -> Shell {
    Shell {
        git: Box::new(Unreached),
        origin: Box::new(Unreached),
        github: Box::new(Unreached),
        files: Box::new(Files(Rc::clone(state))),
        processes: Box::new(Detached(Rc::clone(state))),
        settings,
    }
}

struct Unreached;

impl Git for Unreached {
    fn default_branch(&self) -> String {
        "main".to_owned()
    }
    fn symbolic_ref(&self, _name: &str) -> Option<String> {
        None
    }
    fn valid_branch_name(&self, _branch: &str) -> bool {
        false
    }
    fn has_commit(&self, _oid: &str) -> bool {
        false
    }
    fn is_ancestor(&self, _ancestor: &str, _descendant: &str) -> bool {
        false
    }
    fn executable_hook(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn setting(&self, _remote_ref: &str, _key: &str) -> String {
        String::new()
    }
    fn push_url(&self) -> Option<String> {
        None
    }
    fn current_branch(&self) -> Option<String> {
        None
    }
    fn toplevel(&self) -> Option<String> {
        None
    }
    fn common_dir(&self) -> Step<String> {
        Err(Refused(1))
    }
    fn valid_reference(&self, _reference: &str) -> bool {
        false
    }
    fn resolve(&self, _reference: &str) -> Option<String> {
        None
    }
    fn rev_parse(&self, _revision: &str) -> Step<String> {
        Err(Refused(1))
    }
    fn object_type(&self, _oid: &str) -> Option<String> {
        None
    }
    fn object(&self, _oid: &str) -> Step<Vec<u8>> {
        Err(Refused(1))
    }
    fn commit_of(&self, _revision: &str) -> Step<String> {
        Err(Refused(1))
    }
    fn boundary(&self, _revisions: &[String]) -> Step<String> {
        Err(Refused(1))
    }
    fn count(&self, _revisions: &[String]) -> Step<String> {
        Err(Refused(1))
    }
    fn dirty(&self) -> Step<bool> {
        Err(Refused(1))
    }
    fn annotate(&self, _tag: &str, _annotation: &Annotation, _commit: &str) -> Step {
        Err(Refused(1))
    }
    fn delete_ref(&self, _reference: &str) {}
    fn mirror_holds(&self, _mirror: &Path, _commit: &str, _patterns: &[&str]) -> bool {
        false
    }
}

impl Origin for Unreached {
    fn snapshot(&self, _url: &str, _destination_ref: &str) -> Result<Snapshot, String> {
        Err("recorded shell has no origin".to_owned())
    }
    fn head(&self, _url: &str) -> Option<String> {
        None
    }
    fn tip(&self, _url: &str, _reference: &str) -> Option<String> {
        None
    }
    fn symref(&self, _url: &str, _reference: &str) -> Option<String> {
        None
    }
    fn listing(&self, _url: &str, _references: &[&str]) -> Step<String> {
        Err(Refused(1))
    }
    fn fetch(&self, _refspec: &str) -> Step {
        Err(Refused(1))
    }
    fn fetch_scratch(&self, _url: &str, _refspec: &str) -> bool {
        false
    }
    fn push(&self, _push: &Push) -> Step {
        Err(Refused(1))
    }
}

impl GitHub for Unreached {
    fn public(&self, _url: &str) -> bool {
        false
    }
    fn release_exists(&self, _tag: &str) -> bool {
        false
    }
    fn create_release(&self, _tag: &str) -> Option<bool> {
        None
    }
}

struct Files(Shared);

impl StateFiles for Files {
    fn record_push(&self, _pushed: &Pushed) -> std::io::Result<bool> {
        Ok(true)
    }
    fn is_directory(&self, _path: &Path) -> bool {
        false
    }
    fn append_event(&self, line: &str) -> std::io::Result<()> {
        self.0.borrow_mut().events.push(line.to_owned());
        Ok(())
    }
    fn append_stage(&self, row: &str) -> std::io::Result<()> {
        self.0.borrow_mut().stages.push(row.to_owned());
        Ok(())
    }
    fn output_logs(&self) -> Vec<PathBuf> {
        self.0.borrow().logs.clone()
    }
    fn lock_exclusive(&self, path: &Path) -> Lock {
        let mut state = self.0.borrow_mut();
        if state.unavailable.iter().any(|candidate| candidate == path) {
            return Lock::Unavailable;
        }
        if state.busy.iter().any(|candidate| candidate == path) {
            return Lock::Busy;
        }
        state.locked.push(path.to_path_buf());
        Lock::Held(Box::new(()))
    }
}

struct Detached(Shared);

impl Processes for Detached {
    fn on_path(&self, _program: &str) -> bool {
        false
    }
    fn run_with_input(
        &self,
        _program: &Path,
        _directory: &Path,
        _arguments: &[&str],
        _input: &str,
    ) -> std::io::Result<Step> {
        Ok(Ok(()))
    }
    fn exit_code(&self, _program: &str, _arguments: &[&str]) -> Option<i32> {
        None
    }
    fn current_exe(&self) -> PathBuf {
        PathBuf::from("north")
    }
    fn run_with_outcome(&self, _child: &Child) -> Finished {
        Finished { step: Ok(()), outcome: String::new() }
    }
    fn succeeds(&self, _program: &Path, _directory: &Path, _arguments: &[&str]) -> Option<String> {
        None
    }
    fn capture_output(&self, run: &mut dyn FnMut()) -> String {
        run();
        String::new()
    }
    fn spawn_detached(&self, program: &Path, arguments: &[&str]) {
        self.0.borrow_mut().detached.push((
            program.to_path_buf(),
            arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
        ));
    }
}
