use std::ffi::OsString;
use std::path::Path;

use super::project;

/// north#1521: a lander candidate whose common dir is the bare mirror `north/trusted/repo.git` built cold at `code/repo.git/sandbox`, a path every project's mirror shared.
#[test]
fn a_mirror_candidate_builds_at_its_project_s_sandbox_path() {
    let code = Path::new("/home/tom/code");
    let named = |common: &str| project(code, Path::new(common));
    assert_eq!(named("/home/tom/code/north/trusted/repo.git"), OsString::from("north"));
    assert_eq!(named("/home/tom/code/wisp/trusted/repo.git"), OsString::from("wisp"));
    assert_eq!(named("/home/tom/code/south/main/.git"), OsString::from("south"));
}
