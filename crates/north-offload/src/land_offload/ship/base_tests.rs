use super::tests::{git, shell};
use super::{CommitId, Land, SlotClass, bundle, fill_part, job_ref, quote};
use std::fs;
use std::process::Command;

fn land<'a>(checkout: &'a std::path::Path, base: &CommitId, head: &CommitId) -> Land<'a> {
    Land { ssh: "", checkout, base: Some(base.clone()), head: head.clone(), timeout_seconds: 1, command: "", outputs: &[], class: SlotClass::LandCheck, confined: false, hold: super::Hold::None }
}

#[test]
fn a_base_the_heads_history_does_not_contain_reaches_a_host_copy_cut_before_it_and_a_base_that_is_the_head_ships_the_head_alone() {
    let lane = tempfile::tempdir().unwrap();
    git(lane.path(), &["init", "-q", "-b", "main"]);
    git(lane.path(), &["commit", "-q", "--allow-empty", "-m", "shared"]);
    let host = tempfile::tempdir().unwrap();
    git(host.path(), &["init", "-q"]);
    git(host.path(), &["fetch", "-q", lane.path().to_str().unwrap(), "main"]);
    git(lane.path(), &["commit", "-q", "--allow-empty", "-m", "landed meanwhile"]);
    let base = CommitId::resolve(lane.path(), "HEAD").unwrap();
    git(lane.path(), &["checkout", "-q", "-b", "work", "HEAD~1"]);
    git(lane.path(), &["commit", "-q", "--allow-empty", "-m", "lane"]);
    let head = CommitId::resolve(lane.path(), "HEAD").unwrap();
    let reference = job_ref(&head);
    let shipped = host.path().join("job.bundle");
    let mut file = bundle(&land(lane.path(), &base, &head), &reference, None).unwrap();
    std::io::copy(&mut file, &mut fs::File::create(&shipped).unwrap()).unwrap();
    let script = format!("set -eu\nrepo={} bundle={} ref={} base={} head={} name=job\n{}fill\ngit -C \"$repo\" cat-file -e \"$base^{{commit}}\"\n", shell(host.path()), shell(&shipped), quote(&reference), base.as_str(), head.as_str(), fill_part());
    let ran = Command::new("sh").args(["-c", &script]).output().unwrap();
    assert!(ran.status.success(), "the host lacks the base: {}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(git(lane.path(), &["for-each-ref", "refs/land-offload/"]), "");
    let own = bundle(&land(lane.path(), &head, &head), &reference, None).unwrap();
    let listed = Command::new("git").current_dir(lane.path()).args(["bundle", "list-heads", "-"]).stdin(own).output().unwrap();
    assert_eq!(String::from_utf8(listed.stdout).unwrap().trim(), format!("{} {reference}", head.as_str()));
}
