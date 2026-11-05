use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::queue;

use super::{Fixture, executable, git};

fn kill_lander_hook(fixture: &Fixture) {
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\nkill -9 \"$(awk '{print $4}' /proc/$PPID/stat)\"\nkill -9 $PPID\n");
}

fn three_lanes(fixture: &mut Fixture) -> Vec<PathBuf> {
    ["a", "b", "c"]
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect()
}

#[test]
fn serve_landed_only_on_main() {
    let mut fixture = Fixture::new();
    three_lanes(&mut fixture);
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    let mut sent: Vec<String> = fixture.read("sent").lines().map(str::to_owned).collect();
    sent.sort();
    let expected: Vec<String> = ["a", "b", "c"].iter().map(|name| format!("session-lane-{name} landed lane-{name} as {main}")).collect();
    assert_eq!(sent, expected);
    let cherries = fixture.read("send-cherry");
    assert_eq!(cherries.lines().count(), 3);
    for line in cherries.lines() {
        assert!(!line.contains('+'), "a landed message went out while the lane commit was not on main: {line}");
        assert!(line.contains('-'), "{line}");
    }
    let rows = fixture.pushes_tsv();
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row.len(), 6, "{row:?}");
        assert!(row[0].parse::<u64>().is_ok());
        assert_eq!(row[1], "origin");
        assert_eq!(row[4], main);
        assert_eq!(row[5], format!("session-{}", row[2]));
    }
    let stages = fs::read_to_string(fixture.state.join("safe-push/stages.tsv")).unwrap();
    assert_eq!(stages.lines().filter(|line| line.split('\t').nth(3) == Some("check")).count(), 3);
    assert_eq!(stages.lines().filter(|line| line.split('\t').nth(3) == Some("push")).count(), 3);

    let mut refused = Fixture::new();
    let lane = refused.lane("a", "a.txt", "a");
    refused.enqueue(&lane, None);
    fs::write(refused.trace.join("verdict"), "refused https://github.com/o/r/actions/runs/7").unwrap();
    assert!(refused.serve().status.success());
    let told = refused.fleet();
    assert!(told.len() == 1 && told[0].starts_with("fleet send --from service:north-land session-lane-a north land: lane-a ") && told[0].contains("did not land"), "nothing landed, so the lane is told only its refusal: {told:?}");
    assert!(followed(&refused).is_empty(), "nothing landed, so nothing is followed");
    assert!(refused.pushes_tsv().is_empty());
}

fn followed(fixture: &Fixture) -> Vec<String> {
    fixture.read("north-calls").lines().filter(|line| !line.starts_with("fleet") && !line.starts_with("land-train") && !line.starts_with("capacity offload reap") && !line.starts_with("incident")).map(str::to_owned).collect()
}

#[test]
fn serve_after_landing() {
    let mut fixture = Fixture::new();
    three_lanes(&mut fixture);
    assert!(fixture.serve().status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while followed(&fixture).len() < 4 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let main = fixture.main();
    let (post_land, calls): (Vec<String>, Vec<String>) = followed(&fixture).into_iter().partition(|call| call.starts_with("land checks --after "));
    assert_eq!(post_land.len(), 1, "the landed sha gets one post-land check: {post_land:?} {calls:?}");
    assert!(post_land[0].contains(&format!("post-land-{}.outcome", &main[..12])), "the post-land check runs on the landed sha: {post_land:?}");
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(calls[0].starts_with("after-landing --git-dir ") && calls[0].contains(&format!("--base {} --head {main}", fixture.base)), "{calls:?}");
    assert!(calls[1].starts_with("follows-release --repo-dir "), "{calls:?}");
    assert!(calls[2].starts_with("runtime follow --repo-dir "), "{calls:?}");
    assert_eq!(fixture.fleet().len(), 3);
    assert!(fixture.serve().status.success());
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(followed(&fixture).len(), 4, "an idle round follows and checks nothing again");
}

#[test]
fn serve_restart() {
    let mut fixture = Fixture::new();
    let tickets = three_lanes(&mut fixture);
    kill_lander_hook(&fixture);
    let killed = fixture.serve();
    assert_eq!(killed.status.code(), None, "the lander was killed mid-check");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "");
    assert!(tickets.iter().all(|ticket| ticket.exists()));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "nothing is pushed twice");
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
}

#[test]
fn serve_restart_after_push() {
    let mut fixture = Fixture::new();
    let tickets = three_lanes(&mut fixture);
    fs::write(fixture.trace.join("kill-after-push"), "").unwrap();
    let killed = fixture.serve();
    assert_eq!(killed.status.code(), None, "the lander was killed right after the push");
    let main = fixture.main();
    assert_ne!(main, fixture.base);
    assert!(tickets.iter().all(|ticket| ticket.exists()), "the lander died before it decided the tickets");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "nothing is pushed twice");
    assert_eq!(fixture.checks().len(), 1, "the restarted lander does not check the landed lanes again");
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
}

#[test]
fn serve_reentry() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    executable(
        &fixture.trace.join("hook"),
        &format!(
            "#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\ntimeout 20 {} land enqueue >\"$LAND_TRACE/reentry-enqueue\" 2>&1\necho $? >\"$LAND_TRACE/reentry-status\"\ncommand git push -q origin HEAD:refs/heads/scratch/x\n",
            env!("CARGO_BIN_EXE_north")
        ),
    );
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.read("reentry-status").trim(), "0", "{}", fixture.read("reentry-enqueue"));
    assert!(fixture.read("reentry-enqueue").contains("not queued"), "{}", fixture.read("reentry-enqueue"));
    assert!(queue::pending(&fixture.queue()).unwrap().is_empty(), "the nested enqueue left a ticket");
    assert_eq!(git(&fixture.origin, &["rev-parse", "refs/heads/scratch/x"]), git(&fixture.origin, &["rev-parse", "refs/heads/main"]));
    assert_eq!(fixture.subjects(), ["change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

struct Serving(std::process::Child);

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_exit(serving: &mut Serving, seconds: u64) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        if let Some(status) = serving.0.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

#[test]
fn serve_loop_exits_after_runtime_follow() {
    let mut fixture = Fixture::new();
    let link = fixture.root.join("runtime-current");
    std::os::unix::fs::symlink("gen-1", &link).unwrap();
    let path = format!("{}:{}", fixture.bin.display(), std::env::var("PATH").unwrap());
    let child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["land", "serve", "--repo", fixture.repo.to_str().unwrap(), "--checker", fixture.trace.join("checker").to_str().unwrap()])
        .env("XDG_STATE_HOME", &fixture.state)
        .env("NORTH_RUNTIME_LINK", &link)
        .env("PATH", path)
        .env("LAND_TRACE", &fixture.trace)
        .env("NORTH_LAND_NORTH", fixture.trace.join("north"))
        .env("LAND_ORIGIN", &fixture.origin)
        .env("LAND_REPO", &fixture.repo)
        .env("SOPS_AGE_KEY_FILE", fixture.root.join("age.key"))
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut serving = Serving(child);
    assert!(wait_for_exit(&mut serving, 5).is_none(), "an empty queue keeps the lander serving");
    let lane = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&lane, None);
    let status = wait_for_exit(&mut serving, 120).expect("the lander exits once the follow step changed the runtime");
    assert_eq!(status.code(), Some(0));
    assert_eq!(fixture.subjects(), ["change from a"]);
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("gen-2"));
    assert!(followed(&fixture).iter().any(|call| call.starts_with("runtime follow")));
}

#[test]
fn serve_worktree_not_reaped() {
    let fixture = Fixture::new();
    let lanes = fixture.root.join("proj/worktrees");
    let spend = |name: &str| {
        let path = lanes.join(name);
        git(&fixture.repo, &["worktree", "add", "-q", "--detach", path.to_str().unwrap(), "origin/main"]);
        fs::File::options().write(true).open(path.join(".git")).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600)).unwrap();
    };
    for name in ["lander", "lander-2", "spent-reap"] {
        spend(name);
    }

    let reap = Command::new(env!("CARGO_BIN_EXE_north")).arg("wt-reap").current_dir(&fixture.repo).output().unwrap();
    assert!(reap.status.success(), "{}", String::from_utf8_lossy(&reap.stderr));
    assert!(!lanes.join("spent-reap").exists(), "the control lane is reaped: {}", String::from_utf8_lossy(&reap.stdout));
    assert!(lanes.join("lander/.git").exists() && lanes.join("lander-2/.git").exists());

    spend("spent-gc");
    let gc = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["lane-gc", "--root", fixture.root.to_str().unwrap(), "--idle-hours", "0", "--state", fixture.root.join("unlanded.txt").to_str().unwrap()])
        .env("HOME", &fixture.root)
        .output()
        .unwrap();
    let report = format!("{}{}", String::from_utf8_lossy(&gc.stdout), String::from_utf8_lossy(&gc.stderr));
    assert!(!lanes.join("spent-gc").exists(), "the control lane is collected: {report}");
    assert!(lanes.join("lander/.git").exists() && lanes.join("lander-2/.git").exists(), "{report}");
}
