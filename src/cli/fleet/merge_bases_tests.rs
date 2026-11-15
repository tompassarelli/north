use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use super::*;

#[derive(Default)]
struct Git {
    asked: Mutex<Vec<String>>,
}

impl Evidence for Git {
    fn landed(&self, _cwd: &Path, sha: &str) -> bool {
        self.asked.lock().unwrap().push(sha.to_owned());
        sha.starts_with('a')
    }

    fn running(&self, _records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        BTreeMap::new()
    }

    fn lanes(&self) -> BTreeMap<String, Vec<Lane>> {
        BTreeMap::new()
    }

    fn lane(&self, _record: &SessionRecord) -> Option<Unlanded> {
        None
    }
}

#[test]
fn a_reaper_tick_reuses_merge_bases_of_unmoved_lanes() {
    let state = tempfile::tempdir().unwrap();
    let file = state.path().join("merge-bases.json");
    let git = Git::default();
    let main = Mutex::new("m1".to_owned());
    let read = |_: &Path| Some(main.lock().unwrap().clone());
    let cwd = Path::new("/code/north/main");
    let tick = |git: &Git| {
        let remembered = MergeBases::new(git, &read, file.clone());
        let answers = (remembered.landed(cwd, "a1"), remembered.landed(cwd, "b1"));
        remembered.keep();
        answers
    };

    assert_eq!(tick(&git), (true, false));
    assert_eq!(tick(&git), (true, false));
    assert_eq!(*git.asked.lock().unwrap(), ["a1", "b1"], "an unmoved main asks git nothing again");

    *main.lock().unwrap() = "m2".into();
    assert_eq!(tick(&git), (true, false));
    assert_eq!(*git.asked.lock().unwrap(), ["a1", "b1", "b1"], "a moved main asks again only for what was not yet on it");
}
