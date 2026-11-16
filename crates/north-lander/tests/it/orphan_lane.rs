use std::fs;
use std::path::Path;

use north_foundation::foundation::shim::write_executable;
use north_base::platform::finish_record::{FinishRecords, Finished};
use north_lander::land::adopt::{self, Adopter, Inputs, Lane, Refused, Sessions};
use north_lander::land::queue;

use super::land_first::{NO_SECRETS, PUBLIC_REPO, enqueue};
use super::serve::{HOST, Repository, World, git, report};

struct Retired(bool);

impl Sessions for Retired {
    fn live(&self, _session: &str, _lane: &Path) -> bool {
        self.0
    }
}

struct Lander<'a> {
    world: &'a World,
    north: &'a Repository,
}

impl Adopter for Lander<'_> {
    fn land(&self, lane: &Lane) -> Result<String, Refused> {
        let queued = enqueue(self.world, &lane.root);
        if !queued.status.success() {
            return Err(Refused { summary: "the lane did not queue".to_owned(), detail: report(&queued) });
        }
        let served = self.world.lander(self.north, None).wait_with_output().unwrap();
        let done = self.north.queue.join("done");
        let verdict = fs::read_dir(&done).into_iter().flatten().flatten().filter_map(|entry| queue::read(&entry.path()).ok()).find(|ticket| ticket.root == lane.root).and_then(|ticket| ticket.verdict);
        verdict.filter(|verdict| verdict.starts_with("landed ")).ok_or_else(|| Refused { summary: "the lander did not land the lane".to_owned(), detail: report(&served) })
    }

    fn file(&self, _lane: &Lane, why: &str) -> Result<String, String> {
        panic!("the adopted lane did not land: {why}");
    }
}

#[test]
fn orphan_lane_of_a_retired_session_lands_with_no_session() {
    let world = World::new();
    let north = world.repository("north");
    let bin = world.path().join("bin");
    write_executable(&bin.join("judge"), "#!/bin/sh\ncat > /dev/null\nexit 0\n");
    write_executable(&bin.join("betterleaks"), NO_SECRETS);
    write_executable(&bin.join("gh"), PUBLIC_REPO);
    fs::write(world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    fs::write(world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();

    let lane = north.checkout.parent().unwrap().join("worktrees").join("retired");
    git(&north.checkout, &["worktree", "add", "-q", "-b", "retired", lane.to_str().unwrap(), "origin/main"]);
    fs::write(lane.join("finished.txt"), "the retired worker's finished change\n").unwrap();
    git(&lane, &["add", "-A"]);
    git(&lane, &["commit", "-q", "-m", "Finish the retired worker's box"]);
    let head = git(&lane, &["rev-parse", "HEAD"]);
    fs::write(north.checkout.join("moved.txt"), "main moved after the lane was cut\n").unwrap();
    git(&north.checkout, &["add", "-A"]);
    git(&north.checkout, &["commit", "-q", "-m", "Move main"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);

    let marker = world.path().join("fsmonitor-ran");
    write_executable(&bin.join("fsmonitor"), format!("#!/bin/sh\ntouch {}\n", marker.display()));
    let planted = world.path().join("planted");
    git(world.path(), &["init", "-q", planted.to_str().unwrap()]);
    fs::write(planted.join("planted.txt"), "a worker's planted repository\n").unwrap();
    git(&planted, &["add", "-A"]);
    git(&planted, &["commit", "-q", "-m", "Plant"]);
    git(&planted, &["config", "core.fsmonitor", bin.join("fsmonitor").to_str().unwrap()]);
    let planted_head = git(&planted, &["rev-parse", "HEAD"]);
    let decoy = lane.parent().unwrap().join("planted");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(decoy.join(".git"), format!("gitdir: {}\n", planted.join(".git").display())).unwrap();

    let records = FinishRecords(world.path().join("state/north/verdict-cache/finished"));
    records.record(&Finished { session: "retired-1".to_owned(), command: "true".to_owned(), head, passed: true, at: 1, refused: Vec::new() }).unwrap();
    records.record(&Finished { session: "planted-1".to_owned(), command: "true".to_owned(), head: planted_head, passed: true, at: 2, refused: Vec::new() }).unwrap();
    let inputs = Inputs { code: world.path().to_path_buf(), state: world.path().join("state"), host: HOST.to_owned(), records, jail: Vec::new() };

    assert!(adopt::orphans(&inputs, &Retired(true)).is_empty(), "a lane whose session is live was adopted");
    assert!(!marker.exists(), "git ran the fsmonitor of a repository a lane's planted gitfile names");
    assert_eq!(adopt::orphans(&inputs, &Retired(false)).len(), 1, "the retired session's finished lane was not found");
    git(&north.checkout, &["config", "extensions.worktreeConfig", "true"]);
    assert!(adopt::orphans(&inputs, &Retired(false)).is_empty(), "a lane was adopted from a repository with per-worktree config on");
    git(&north.checkout, &["config", "--unset", "extensions.worktreeConfig"]);
    let lines = adopt::adopt(&inputs, &Retired(false), &Lander { world: &world, north: &north }, false);
    assert!(lines.iter().any(|line| line.contains("landed ")), "{lines:?}");
    let subjects = git(&north.origin, &["log", "--format=%s", "main"]);
    assert!(subjects.lines().any(|subject| subject == "Finish the retired worker's box") && subjects.lines().any(|subject| subject == "Move main"), "{subjects}");
    assert!(adopt::orphans(&inputs, &Retired(false)).is_empty(), "a landed lane was offered for adoption again");
}
