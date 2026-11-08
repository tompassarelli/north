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
    let main = south.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["remote", "add", "origin", "https://github.com/tompassarelli/south.git"]);
    let denylist = Denylist { south, config: None };
    assert!(denylist.is_south("https://github.com/tompassarelli/south.git"));
    assert_eq!(denylist.visibility_of("git@github.com:tompassarelli/south.git"), "PRIVATE");
    assert_eq!(denylist.visibility_of("https://github.com/TomPassarelli/south"), "PRIVATE");
    assert!(!denylist.is_south("https://github.com/tompassarelli/north.git"));
    assert!(!Denylist { south: dir.path().join("absent"), config: None }.is_south("https://github.com/tompassarelli/south.git"));
}

// KEEP(bug): the private-text judge's 300 s timeout refused north#868 twice on 2026-11-08, although a judge with no verdict says nothing about the lane; north#842
#[test]
fn a_judge_that_times_out_holds_the_push_as_no_verdict_not_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("main")).unwrap();
    let denylist = Denylist { south: dir.path().to_path_buf(), config: None };
    let judged = |judge: Result<Vec<Finding>, String>| denylist.verdict(|| "PUBLIC".to_owned(), || Ok(BTreeSet::new()), || judge).stand;
    assert_eq!(judged(Err("the private-text judge did not finish".to_owned())), Stand::NoVerdict);
    assert_eq!(judged(Err("the private-text judge could not run (exit 2): no model".to_owned())), Stand::NoVerdict);
    assert_eq!(judged(Ok(vec![Finding { kind: "address".to_owned(), span: "x".to_owned() }])), Stand::Refuse, "a judge that names a private kind still refuses");
    assert_eq!(judged(Ok(Vec::new())), Stand::Pass);
}
