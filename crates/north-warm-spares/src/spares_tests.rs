use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::fake_host::{holding, machine};
use crate::host::{HostSpec, held_dir};
use crate::machine::Machine;
use crate::spares::{self, Place};

const WAIT: Duration = Duration::from_secs(20);

fn spare_in(place: &Place, cwd: &Path, name: &str) {
    let spec = HostSpec { key: name.into(), shared: false, argv: vec!["sleep".into(), "600".into()], env: Vec::new(), env_remove: Vec::new(), cwd: cwd.to_path_buf() };
    spares::start(place, &spares::spare_spec(spec, place, name), "kind", "fp", name, &format!("conversation-{name}")).unwrap();
}

fn pooled(place: &Place) -> Vec<String> {
    spares::survey(place).unwrap().entries.into_iter().map(|entry| entry.rsplit('-').next().unwrap().to_owned()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spare_whose_directory_was_removed_is_retired_by_the_next_survey_and_its_slot_counts_free() {
    let root = tempfile::tempdir().unwrap();
    let shims = machine(root.path());
    let (far, home) = (Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &shims.join("ssh"), machine: &far, home };
    let (kept, gone) = (home.join("lanes/kept"), home.join("lanes/gone"));
    fs::create_dir_all(&kept).unwrap();
    fs::create_dir_all(&gone).unwrap();
    spare_in(&place, &kept, "spare-kept");
    spare_in(&place, &gone, "spare-gone");
    assert_eq!(pooled(&place).len(), 2);

    fs::remove_dir_all(&gone).unwrap();
    assert_eq!(pooled(&place), vec!["kept".to_owned()], "the survey retires the spare whose directory is gone, so it fills no slot");
    let log = fs::read_to_string(home.join(".local/state/north/spares-retired.log")).unwrap();
    assert!(log.lines().count() == 1 && log.contains("spare-gone age ") && log.contains("retired unused"), "one line names the spare, its age and why: {log}");

    fs::remove_dir_all(&kept).unwrap();
    fs::create_dir_all(&kept).unwrap();
    assert_eq!(pooled(&place), Vec::<String>::new(), "a directory made again under the same name is not the worktree the spare started in");
    let quiet = spares::Pool { memory_kb: u64::MAX, ..spares::survey(&place).unwrap() };
    assert_eq!(spares::wanted(&quiet, "kind", "fp", None, false).1, spares::PER_KIND, "the retired spares' slots are free");
    let full = spares::Pool { memory_kb: u64::MAX, entries: (0..spares::PER_HOST).map(|n| format!("other{}-fp-spare-{n}", n % 2)).collect(), started_s: (0..spares::PER_HOST as u64).collect() };
    assert_eq!(spares::wanted(&full, "kind", "fp", None, false), (vec![], 0), "a refresh retires no other kind's spares");
    assert_eq!(spares::wanted(&full, "kind", "fp", None, true), (full.entries[..spares::PER_KIND].to_vec(), spares::PER_KIND), "a launch's fill retires the oldest spares of other kinds to make room for its own (north#1531)");
    let held = home.join(".local/state/north/held");
    north_base::platform::wait::until_async(WAIT, || holding(&held).is_empty()).await;
    assert_eq!(holding(&held), Vec::<String>::new(), "retiring leaves no process of the spare behind");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retiring_a_directory_tree_stops_every_spare_under_it_and_none_outside() {
    let root = tempfile::tempdir().unwrap();
    let shims = machine(root.path());
    let (far, home) = (Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &shims.join("ssh"), machine: &far, home };
    let tree = home.join("eval/worktrees");
    let (one, two, outside) = (tree.join("one"), tree.join("two"), home.join("eval/worktrees-other"));
    for dir in [&one, &two, &outside] {
        fs::create_dir_all(dir).unwrap();
    }
    spare_in(&place, &one, "spare-one");
    spare_in(&place, &two, "spare-two");
    spare_in(&place, &outside, "spare-out");

    assert_eq!(spares::retire_under(&place, &home.join("empty")), Some(Vec::new()), "a tree with no spare retires nothing");
    let lines = spares::retire_under(&place, &tree).unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines.iter().all(|line| line.contains("age ") && line.contains(&format!("directory tree {} was retired", tree.display()))), "{lines:?}");
    assert_eq!(pooled(&place), vec!["out".to_owned()], "the spare outside the tree stays");
    assert_eq!(spares::retire_under(&place, Path::new("relative")), None);
    let of = |spare: &str| held_dir(home, &format!("north-{spare}.service"));
    north_base::platform::wait::until_async(WAIT, || holding(&of("spare-one")).is_empty() && holding(&of("spare-two")).is_empty()).await;
    assert!(holding(&of("spare-one")).is_empty() && holding(&of("spare-two")).is_empty(), "no process of a retired spare remains");
    assert!(!holding(&of("spare-out")).is_empty(), "the outside spare still runs");
}
