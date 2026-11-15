use std::ffi::OsString;
use std::path::Path;

use super::{Mode, Sandbox, project};

fn bound(mode: Mode, root: &Path) -> Vec<String> {
    let mut sandbox = Sandbox {
        mode,
        top: root.to_path_buf(),
        sandbox: root.join("sandbox"),
        home: root.to_path_buf(),
        named_home: root.to_path_buf(),
        runtime: root.join("runtime"),
        arguments: Vec::new(),
        binds: Vec::new(),
        hand_off: None,
        carried: Vec::new(),
        scratch: None,
        held: None,
    };
    sandbox.bind_caches().unwrap();
    sandbox.arguments.iter().map(|word| word.to_string_lossy().into_owned()).collect()
}

#[test]
fn a_run_sandbox_keeps_nix_s_cache_out_of_the_shared_bind_and_a_fetch_binds_it_writable() {
    let home = tempfile::tempdir().unwrap();
    let nix = home.path().join(".cache/nix").to_string_lossy().into_owned();
    let run = bound(Mode::Run, home.path());
    assert!(run.windows(2).any(|pair| pair[0] == "--tmpfs" && pair[1] == nix));
    assert!(!run.windows(3).any(|group| group[0] == "--bind" && group[1] == nix));
    let fetch = bound(Mode::Fetch, home.path());
    assert!(fetch.windows(3).any(|group| group[0] == "--bind" && group[1] == nix && group[2] == nix));
}

/// north#1521: a lander candidate whose common dir is the bare mirror `north/trusted/repo.git` built cold at `code/repo.git/sandbox`, a path every project's mirror shared.
#[test]
fn a_mirror_candidate_builds_at_its_project_s_sandbox_path() {
    let code = Path::new("/home/tom/code");
    let named = |common: &str| project(code, Path::new(common));
    assert_eq!(named("/home/tom/code/north/trusted/repo.git"), OsString::from("north"));
    assert_eq!(named("/home/tom/code/wisp/trusted/repo.git"), OsString::from("wisp"));
    assert_eq!(named("/home/tom/code/south/main/.git"), OsString::from("south"));
}
