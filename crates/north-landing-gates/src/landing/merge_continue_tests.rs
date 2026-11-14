use std::fs;
use std::path::Path;
use std::process::Command;

use super::refresh;
use crate::landing::merge_set;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

#[test]
fn derived_files_regenerate_at_landing_from_a_lane_that_changes_only_their_inputs() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "lander"]);
    git(root, &["config", "user.email", "lander@localhost"]);
    let list = "[[set]]\npath = \"crates/**/BUCK\"\nkind = \"regenerate\"\ncommand = \"sh targets.sh\"\nsources = [\"**/Cargo.toml\"]\n";
    let generator = "for manifest in crates/*/Cargo.toml; do cp \"$manifest\" \"${manifest%Cargo.toml}BUCK\"; done\n";
    let main = commit(root, &[("harness/merge-sets.toml", list), ("targets.sh", generator), ("crates/docs/Cargo.toml", "serde_json\n"), ("crates/docs/BUCK", "serde_json\n")], "main");
    let jail = tempfile::tempdir().unwrap();
    let north = jail.path().join("north");
    north_base::foundation::shim::write_script(&north, "[ \"$1 $2 $3\" = \"--worker-sandbox --candidate --\" ] || exit 9\nshift 3\nexec \"$@\"\n");
    merge_set::run_drivers_with(&north);

    commit(root, &[("crates/docs/Cargo.toml", "serde_json\ntoml_edit\n")], "a lane adds a dependency to a manifest and leaves its BUCK file stale");
    assert_eq!(refresh(root, &main, &main).unwrap(), ["crates/**/BUCK"]);
    assert_eq!(git(root, &["show", "HEAD:crates/docs/BUCK"]), "serde_json\ntoml_edit", "the landing commit does not carry the BUCK file the generator writes from the lane's manifest");
    assert_eq!(git(root, &["rev-list", "--count", &format!("{main}..HEAD")]), "1", "the regenerated file is not folded into the lane's own commit");

    let landed = git(root, &["rev-parse", "HEAD"]);
    commit(root, &[("notes.md", "a lane that changes no generator input\n"), ("targets.sh", "exit 3\n")], "a lane outside every input");
    assert!(refresh(root, &main, &landed).unwrap().is_empty(), "a landing that changes no generator input ran a generator");
}
