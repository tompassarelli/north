use std::fs;
use std::path::{Path, PathBuf};

use north_landing::land::queue;

use super::{Fixture, executable, git, json_names, pushed_trees_were_checked};

#[test]
fn remote_outbox_lands() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45\n");
    let home = fixture.root.join("remote-homes/gex45");
    let clone = fixture.root.join("gex45/proj/main");
    fs::create_dir_all(&home).unwrap();
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    git(&clone, &["config", "user.name", "Remote scenario"]);
    git(&clone, &["config", "user.email", "remote@example.invalid"]);
    let lane = fixture.root.join("gex45/proj/worktrees/r");
    git(&clone, &["worktree", "add", "-q", "-b", "lane-r", lane.to_str().unwrap(), "origin/main"]);
    fixture.commit(&lane, "r.txt", "r", "change from r");
    let head = git(&lane, &["rev-parse", "HEAD"]);
    let top = git(&lane, &["rev-parse", "--show-toplevel"]);
    let queued = fixture.land_enqueue(&lane, &home.join(".local/state"), "session-remote");
    assert!(queued.status.success());
    assert!(String::from_utf8_lossy(&queued.stdout).contains(&format!("Done: queued (outbox) {top} lane-r {head}")));
    let outbox = queue::directory(&home.join(".local/state"), fixture.origin.to_str().unwrap()).join(queue::OUTBOX);
    let names = json_names(&outbox);
    assert_eq!(names.len(), 1, "{names:?}");

    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"]);
    assert!(json_names(&outbox).is_empty(), "the remote ticket is still in the outbox");
    assert_eq!(json_names(&outbox.join("taken")), names);
    let main = fixture.main();
    let local = fixture.queue().join(&names[0]);
    assert_eq!(fixture.verdict(&local), Some(format!("landed {main}")));
    let remote = queue::read(&outbox.join("taken").join(&names[0])).unwrap();
    let done = queue::read(&fixture.queue().join("done").join(&names[0])).unwrap();
    assert_eq!((done.host.as_str(), done.root.as_path()), (remote.host.as_str(), remote.root.as_path()), "the pulled ticket keeps its host and root");
    assert!(fixture.fleet().contains(&format!("fleet send --from service:north-land session-remote landed lane-r as {main}")), "{:?}", fixture.fleet());
    let ssh = fixture.read("ssh");
    assert!(ssh.lines().all(|line| line.contains("ConnectTimeout=5")), "{ssh}");
    assert!(ssh.lines().any(|line| line.contains("git-upload-pack") && line.contains(&top)), "the lane is fetched from ssh://gex45{top}: {ssh}");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"], "a taken ticket was pulled again");
}

#[test]
fn remote_outbox_unreachable() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tremote-hosts = nexus\n");
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let output = fixture.serve();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line.starts_with("north land: waiting: nexus's outbox skipped this round")), "{stdout}");
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())), "an unreachable host blocked the local ticket");
    assert!(fixture.read("ssh").lines().all(|line| line.contains("ConnectTimeout=5")));
}

impl Fixture {
    fn staged_remote_ticket(&mut self, name: &str, before_local: bool) -> (PathBuf, PathBuf) {
        let home = self.root.join("remote-homes/gex45");
        let clone = self.root.join("gex45/proj/main");
        fs::create_dir_all(&home).unwrap();
        git(&self.root, &["clone", "-q", self.origin.to_str().unwrap(), clone.to_str().unwrap()]);
        git(&clone, &["config", "user.name", "Remote scenario"]);
        git(&clone, &["config", "user.email", "remote@example.invalid"]);
        let lane = self.root.join("gex45/proj/worktrees").join(name);
        git(&clone, &["worktree", "add", "-q", "-b", &format!("lane-{name}"), lane.to_str().unwrap(), "origin/main"]);
        self.commit(&lane, &format!("{name}.txt"), name, &format!("change from {name}"));
        assert!(self.land_enqueue(&lane, &home.join(".local/state"), &format!("session-{name}")).status.success());
        let outbox = queue::directory(&home.join(".local/state"), self.origin.to_str().unwrap()).join(queue::OUTBOX);
        let names = json_names(&outbox);
        assert_eq!(names.len(), 1, "{names:?}");
        let staged = self.root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        let staged_name = if before_local { queue::file_name(self.queued - 1_000_000, &format!("lane-{name}")) } else { names[0].clone() };
        fs::rename(outbox.join(&names[0]), staged.join(&staged_name)).unwrap();
        (staged, outbox)
    }

    fn release_on_check(&self, check: usize, staged: &Path, outbox: &Path, extra: &str) {
        executable(
            &self.trace.join("hook"),
            &format!("#!/usr/bin/env bash\n[ \"$1\" = {check} ] || exit 0\nmv {}/*.json {}/\n{extra}", staged.display(), outbox.display()),
        );
    }
}

#[test]
fn remote_outbox_joins_the_rebuilt_candidate() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45 nexus\n");
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let (staged, outbox) = fixture.staged_remote_ticket("r", false);
    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    fixture.release_on_check(1, &staged, &outbox, &format!("cd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'main moves during the check'\ncommand git push -q origin main\n", racer.display()));
    let output = fixture.serve();
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "the rebuilt candidate carried the ticket pulled after the first check: {checks:?}");
    assert_eq!(fixture.subjects(), ["main moves during the check", "change from a", "change from r"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    assert!(json_names(&outbox).is_empty(), "the remote ticket is still in the outbox");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().filter(|line| line.starts_with("north land: waiting: nexus's outbox skipped this round")).count(), 2, "once per round, and the second round finds the queue empty: {stdout}");
    assert_eq!(fixture.read("ssh").lines().filter(|line| line.contains("nexus")).count(), 3, "the outbox is pulled before each full candidate");
}

#[test]
fn remote_outbox_waits_out_the_bisect() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45\n");
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    let (staged, outbox) = fixture.staged_remote_ticket("r", true);
    fs::write(fixture.trace.join("poison"), "a.txt\nrefused https://github.com/o/r/actions/runs/1\n1\n").unwrap();
    fixture.release_on_check(1, &staged, &outbox, "");
    assert!(fixture.serve().status.success(), "the bisect did not converge");
    assert!(fixture.verdict(&tickets[0]).is_some_and(|verdict| verdict.starts_with("failed ")), "{:?}", fixture.verdict(&tickets[0]));
    let main = fixture.main();
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {main}")));
    let taken = json_names(&outbox.join("taken"));
    assert!(json_names(&outbox).is_empty() && taken.len() == 1, "the pulled ticket was lost: {taken:?}");
    assert_eq!(fixture.verdict(&fixture.queue().join(&taken[0])), Some(format!("landed {main}")));
    let mut subjects = fixture.subjects();
    subjects.sort();
    assert_eq!(subjects, ["change from b", "change from r"], "each ticket lands once");
    let checks = fixture.checks();
    let carrying_r: Vec<_> = checks.iter().filter(|(head, _)| git(&fixture.repo, &["ls-tree", "--name-only", head]).lines().any(|file| file == "r.txt")).collect();
    assert_eq!(carrying_r.len(), 1, "the pulled ticket was checked more than once: {checks:?}");
    assert_eq!(carrying_r[0].0, main, "the pulled ticket landed in a candidate other than the one checked");
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{}..{main}", carrying_r[0].1)]), "2", "the landed candidate sits directly on the main it was checked against");
    pushed_trees_were_checked(&fixture);
}

#[test]
fn remote_enqueue_off_lander_host() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n");
    let a = fixture.lane("a", "a.txt", "a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    let top = git(&a, &["rev-parse", "--show-toplevel"]);
    let output = fixture.land_enqueue(&a, &fixture.state, "session-a");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == format!("Done: queued (outbox) {top} lane-a {head}")));
    assert_eq!(json_names(&fixture.queue().join(queue::OUTBOX)).len(), 1);
    assert!(queue::pending(&fixture.queue()).unwrap().is_empty(), "an off-lander-host ticket went to the lander queue");

    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned();
    fixture.landing_config(&format!("\tlander-host = {host}\n"));
    let output = fixture.land_enqueue(&a, &fixture.state, "session-a");
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == format!("Done: queued {top} lane-a {head}")));
    assert_eq!(queue::pending(&fixture.queue()).unwrap().len(), 1, "the lander host queues locally");
}
