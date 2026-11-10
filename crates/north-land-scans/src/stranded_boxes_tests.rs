use std::path::Path;

use north_github_mirror::store::{Mirror, OPEN_ISSUES, key};
use serde_json::json;

use super::scan;

const REFERENCE: &str = "Commands:\n  fleet dispatch [--dry-run]  Place\n  ops climb run [--dry-run]  Climb\n  ops dividend [CHECK]  Evaluate\n";
const RETIRED: &str = "Commands:\n  fleet dispatch [--dry-run]  Place\n  ops dividend [CHECK]  Evaluate\n";
const STRANDED: &str = "the week's loss prints. Done when: `north ops climb run --dry-run` exits 0";
const ARMS: &str = "arms are read in crates/north-climb/src/arms.rs.";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").current_dir(directory).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@example.invalid").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@example.invalid").args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(directory: &Path, reference: &str, arms: bool) -> String {
    let path = directory.join("crates/north-climb/src/arms.rs");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    if arms { std::fs::write(&path, "").unwrap() } else { std::fs::remove_dir_all(directory.join("crates")).unwrap() }
    std::fs::write(directory.join("src/cli/reference.txt"), reference).unwrap();
    git(directory, &["add", "-A"]);
    git(directory, &["commit", "-qm", "change"]);
    git(directory, &["rev-parse", "HEAD"])
}

fn stranded(directory: &Path, base: &str, head: &str, mirror: &Mirror, body: &str) -> (bool, String) {
    mirror.write(&key("north", OPEN_ISSUES), &json!([{"number": 872, "title": "climb", "body": body}]), None, 1.0).unwrap();
    let mut out = Vec::new();
    let refused = scan(directory, base, head, mirror, &mut out);
    (refused, String::from_utf8(out).unwrap())
}

/// north#1054: bce9c60404 (north#970) deleted `north ops climb` while open boxes on north#872 and north#882 still ran it, and workers spent turns finding out.
#[test]
fn a_removal_naming_an_open_box_check_fails_its_landing() {
    let root = tempfile::tempdir().unwrap();
    let north = root.path().join("north");
    std::fs::create_dir_all(north.join("src/cli")).unwrap();
    git(&north, &["init", "-q", "-b", "main"]);
    git(&north, &["remote", "add", "origin", "git@github.com:tompassarelli/north.git"]);
    let base = commit(&north, REFERENCE, true);
    let head = commit(&north, RETIRED, false);
    let mirror = Mirror::at(root.path().join("github-mirror.db"));

    let (refused, out) = stranded(&north, &base, &head, &mirror, &format!("- [ ] {STRANDED}\n- [ ] {ARMS}\n- [ ] `north fleet dispatch --dry-run` plans"));
    assert!(refused && out.contains("north#872: `north ops climb` in box: the week's loss prints") && out.contains("north#872: `crates/north-climb/src/arms.rs` in box") && !out.contains("plans"), "{out}");

    let (refused, out) = stranded(&north, &base, &head, &mirror, &format!("- [x] {STRANDED}\n- [x] {ARMS}\n- [ ] the week's loss prints. Done when: `north ops dividend` exits 0"));
    assert!(!refused && out.is_empty(), "a ticked or edited box no longer strands: {out}");
}
