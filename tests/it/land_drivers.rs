use std::fs;
use std::path::Path;
use std::process::Command;

use north_landing::land::candidate::{self, Pick};
use north_landing::land::queue::Ticket;
use north_landing::landing::merge_set;

const NORTH: &str = env!("CARGO_BIN_EXE_north");

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_EDITOR", "true").output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn structure(uses: &str) -> String {
    format!("[[module]]\nname = \"core\"\nmay_use = [{uses}]\n")
}

#[test]
fn set_driver_without_shared_config() {
    merge_set::run_drivers_with(Path::new(NORTH));
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("harness")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]);
    }
    fs::write(repo.join("harness/merge-sets.toml"), "[[set]]\npath = \"structure.toml\"\nkind = \"toml-array\"\nkey = \"may_use\"\n").unwrap();
    fs::write(repo.join(".gitattributes"), "structure.toml merge=set\n").unwrap();
    fs::write(repo.join("structure.toml"), structure("\"base\"")).unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let mut tickets = Vec::new();
    for entry in ["fleet", "spawn"] {
        git(&repo, &["checkout", "-q", "-b", entry, &base]);
        fs::write(repo.join("structure.toml"), structure(&format!("\"base\", \"{entry}\""))).unwrap();
        git(&repo, &["commit", "-qam", entry]);
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let ticket = Ticket { root: repo.clone(), branch: entry.to_owned(), head, ..Default::default() };
        tickets.push((dir.path().join(format!("{entry}.json")), ticket));
    }
    let worktree = dir.path().join("lander");
    candidate::ensure(&repo, &worktree).unwrap();
    let shared = Command::new("git").arg("-C").arg(&worktree).args(["config", "--local", "--get-regexp", "^merge\\."]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&shared.stdout), "", "the shared .git/config holds no merge keys");
    let built = candidate::build(&worktree, &base, &tickets, 2, &dir.path().join("logs")).unwrap();
    let reasons = built.picks.iter().map(|pick| match pick { Pick::Included => "included".to_owned(), Pick::Decided(reason) | Pick::Undecided(reason) | Pick::Deferred(reason) => reason.clone() }).collect::<Vec<_>>();
    assert_eq!(built.included(), 2, "{reasons:?}");
    assert_eq!(git(&worktree, &["show", "HEAD:structure.toml"]), structure("\"base\", \"fleet\", \"spawn\"").trim_end());
    assert_eq!(git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]), "2");
}
