use north_base::test_inputs::git::git_in as git;
use std::fs;
use std::path::Path;

use super::{Generators, refresh};
use crate::landing::merge_set;

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
    assert_eq!(refresh(root, &main, &main, Generators::Merged).unwrap(), ["crates/**/BUCK"]);
    assert_eq!(git(root, &["show", "HEAD:crates/docs/BUCK"]), "serde_json\ntoml_edit", "the landing commit does not carry the BUCK file the generator writes from the lane's manifest");
    assert_eq!(git(root, &["rev-list", "--count", &format!("{main}..HEAD")]), "1", "the regenerated file is not folded into the lane's own commit");

    let landed = git(root, &["rev-parse", "HEAD"]);
    commit(root, &[("notes.md", "a lane that changes no generator input\n"), ("targets.sh", "exit 3\n")], "a lane outside every input");
    assert!(refresh(root, &main, &landed, Generators::Merged).unwrap().is_empty(), "a landing that changes no generator input ran a generator");

    north_base::foundation::shim::write_script(&north, "case \"$1 $2\" in\n\"--worker-sandbox --candidate\") shift 3; exec \"$@\" ;;\n\"runtime build-ahead\") echo every build host is over its admission limit >&2; exit 75 ;;\n\"runtime covers\") exit 1 ;;\n\"buck targets\") cp crates/docs/Cargo.toml crates/docs/BUCK ;;\n*) exit 9 ;;\nesac\n");
    let list = "[[set]]\npath = \"crates/**/BUCK\"\nkind = \"regenerate\"\ncommand = \"north buck targets\"\nsources = [\"**/Cargo.toml\"]\ngenerator = [\"targets/**\"]\n";
    let fixed = commit(root, &[("harness/merge-sets.toml", list), ("targets/mod.rs", "old\n")], "main lists a north generator");
    commit(root, &[("targets/mod.rs", "new\n"), ("crates/docs/Cargo.toml", "serde_json\nurgent\n")], "an urgent fix changes the generator code and a manifest");
    assert!(refresh(root, &fixed, &fixed, Generators::Merged).unwrap_err().message.contains("over its admission limit"), "a lane that changes the generator code ran it on the installed north");
    assert_eq!(refresh(root, &fixed, &fixed, Generators::Installed).unwrap(), ["crates/**/BUCK"], "an urgent fix waited on a build of its merged tree instead of the installed north");
    assert_eq!(git(root, &["show", "HEAD:crates/docs/BUCK"]), "serde_json\nurgent");
}
