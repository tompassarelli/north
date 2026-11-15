use std::fs;

use super::*;
use crate::land::review_fetch;
use north_trusted_mirror::{forget_review, lane_name_ok, review_head, review_ref};

fn repository(dir: &Path, origin: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    git(dir, &["clone", "-q", &origin.to_string_lossy(), &path.to_string_lossy()]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&path, &["config", key, value]).unwrap();
    }
    path
}

#[test]
fn a_lane_handed_in_on_a_build_host_is_readable_by_a_confined_reviewer() {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    git(dir.path(), &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]).unwrap();
    let seed = repository(dir.path(), &origin, "seed");
    fs::write(seed.join("f"), "base\n").unwrap();
    git(&seed, &["add", "f"]).unwrap();
    git(&seed, &["commit", "-q", "-m", "base"]).unwrap();
    git(&seed, &["push", "-q", "origin", "HEAD:refs/heads/main"]).unwrap();
    fs::create_dir_all(dir.path().join("laptop/north")).unwrap();
    let main = repository(&dir.path().join("laptop/north"), &origin, "main");
    let build_host = repository(dir.path(), &origin, "gex45");
    git(&build_host, &["checkout", "-q", "-b", "lane"]).unwrap();
    fs::write(build_host.join("f"), "lane\n").unwrap();
    git(&build_host, &["commit", "-q", "-am", "lane"]).unwrap();
    let head = git(&build_host, &["rev-parse", "HEAD"]).unwrap();
    let bundle = dir.path().join("lane.bundle");
    git(&build_host, &["bundle", "create", "-q", &bundle.to_string_lossy(), "refs/heads/lane"]).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(fs::read(&bundle).unwrap());
    let lander = Lander::new(&main, Vec::new()).unwrap();
    let outbox = Outbox { host: "gex45".to_owned(), login: "tom@gex45".to_owned(), agent: false };
    let ticket = Ticket { branch: "lane".to_owned(), head: head.clone(), host: "gex45".to_owned(), root: build_host.clone(), ..Ticket::default() };
    assert!(lander.take_lane(&outbox, &ticket, Some(&encoded)).is_ok());
    let reviewer = repository(dir.path(), &origin, "reviewer");
    north_trusted_mirror::fetch_lane_head(&main, &reviewer, &head).unwrap();
    assert_eq!(git(&reviewer, &["cat-file", "-p", &format!("{head}:f")]).unwrap(), "lane", "a reviewer reads a lane handed in on a build host from the laptop's trusted mirror, with no ssh to that host (north#1485)");
    assert!(lane_name_ok("x;id").is_err(), "a lane name with a shell metacharacter never reaches a path, ref or ssh word");
    assert!(lane_name_ok(&head).is_err() && lane_name_ok("abc1234").is_err(), "a commit name is never a lane name, so a plain accept of a commit never reads refs/review");
    let fetched = review_fetch::fetch_from(&main, "lane", &build_host.to_string_lossy(), "gex45").unwrap();
    assert_eq!(fetched.sha, head, "the review ref holds the head of the lane's worktree, fetched with no ssh");
    assert_eq!(review_head(&main, "lane").unwrap(), Some(head.clone()), "an accept reads the review ref the laptop fetched");
    let accepter = repository(dir.path(), &origin, "accepter");
    north_trusted_mirror::fetch_lane_head(&main, &accepter, &fetched.sha).unwrap();
    assert_eq!(git(&accepter, &["cat-file", "-p", &format!("{}:f", fetched.sha)]).unwrap(), "lane", "a plain full sha that review-fetch printed resolves from the trusted mirror, so an accept names a commit and never reads refs/review (north#1549)");
    north_trusted_mirror::fetch_lane_head(&main, &reviewer, &review_ref("lane")).unwrap();
    assert_eq!(git(&reviewer, &["cat-file", "-p", &format!("{head}:f")]).unwrap(), "lane", "a reviewer reads a lane that was never handed in from refs/review/lane in the trusted mirror, with no patch relayed by message (north#1549)");
    forget_review(&main, "lane").unwrap();
    assert_eq!(review_head(&main, "lane").unwrap(), None, "a landed lane's review ref is gone from the mirror");
}
