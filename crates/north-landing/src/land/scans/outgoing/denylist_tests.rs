use std::path::Path;
use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_push_to_south_itself_is_private_whatever_the_origin_spelling_and_without_asking_github() {
    let dir = tempfile::tempdir().unwrap();
    let south = dir.path().join("south");
    std::fs::create_dir_all(&south).unwrap();
    git(&south, &["init", "-q"]);
    git(&south, &["remote", "add", "origin", "https://github.com/tompassarelli/south.git"]);
    let denylist = Denylist { south, config: None };
    assert_eq!(denylist.visibility_of("git@github.com:tompassarelli/south.git"), "PRIVATE");
    assert_eq!(denylist.visibility_of("https://github.com/TomPassarelli/south"), "PRIVATE");
    assert!(!denylist.is_south("https://github.com/tompassarelli/north.git"));
    assert!(!Denylist { south: dir.path().join("absent"), config: None }.is_south("https://github.com/tompassarelli/south.git"));
}
