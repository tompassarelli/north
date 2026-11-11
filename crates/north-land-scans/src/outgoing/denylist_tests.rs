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
    let denylist = Denylist { south, south_named: false, config: None, kinds: None };
    assert!(denylist.is_south("https://github.com/tompassarelli/south.git"));
    assert_eq!(denylist.visibility_of("git@github.com:tompassarelli/south.git"), "PRIVATE");
    assert_eq!(denylist.visibility_of("https://github.com/TomPassarelli/south"), "PRIVATE");
    assert!(!denylist.is_south("https://github.com/tompassarelli/north.git"));
    assert!(!Denylist { south: dir.path().join("absent"), south_named: false, config: None, kinds: None }.is_south("https://github.com/tompassarelli/south.git"));
}

#[test]
fn a_judge_that_times_out_holds_the_push_as_no_verdict_not_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("main")).unwrap();
    let denylist = Denylist { south: dir.path().to_path_buf(), south_named: false, config: None, kinds: None };
    let judged = |judge: Result<Vec<Finding>, String>| denylist.verdict(|| "PUBLIC".to_owned(), || Ok(BTreeSet::new()), || judge.map(Some)).stand;
    assert_eq!(judged(Err("the private-text judge did not finish".to_owned())), Stand::NoVerdict);
    assert_eq!(judged(Err("the private-text judge could not run (exit 2): no model".to_owned())), Stand::NoVerdict);
    assert_eq!(judged(Ok(vec![Finding { kind: "address".to_owned(), span: "x".to_owned() }])), Stand::Refuse, "a judge that names a private kind still refuses");
    assert_eq!(judged(Ok(Vec::new())), Stand::Pass);
}

#[test]
fn an_unreachable_deny_list_holds_the_push_as_no_verdict_and_a_scan_failure_still_refuses() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("main")).unwrap();
    let denylist = Denylist { south: dir.path().to_path_buf(), south_named: false, config: None, kinds: None };
    let scanned = |findings: Result<BTreeSet<String>, Fault>| denylist.verdict(|| "PUBLIC".to_owned(), || findings, || Ok(Some(Vec::new()))).stand;
    assert_eq!(scanned(Err(Fault::Unavailable("no network".to_owned()))), Stand::NoVerdict);
    assert_eq!(scanned(Err(Fault::Failed("the deny-list is missing".to_owned()))), Stand::Refuse);
    assert_eq!(scanned(Ok(BTreeSet::from(["fake-private-term in notes.txt".to_owned()]))), Stand::Refuse, "a matched private value still refuses");
}

#[test]
fn a_public_push_whose_override_names_a_missing_or_empty_source_is_refused_and_only_a_host_with_no_south_named_passes_unscanned() {
    let dir = tempfile::tempdir().unwrap();
    let (home, empty) = (dir.path().join("home"), dir.path().join("empty.toml"));
    std::fs::write(&empty, "\n").unwrap();
    let missing = dir.path().join("no-south");
    let no_main = dir.path().join("south-without-main");
    std::fs::create_dir_all(&no_main).unwrap();
    let empty_kinds = dir.path().join("empty-kinds.toml");
    std::fs::write(&empty_kinds, "\n").unwrap();
    let stand = |vars: &[(&str, &Path)]| {
        let denylist = Denylist::from_vars(|name| vars.iter().chain([&("HOME", home.as_path())]).find(|(set, _)| *set == name).map(|(_, value)| value.as_os_str().to_owned()));
        denylist.verdict(|| "PUBLIC".to_owned(), || Ok(BTreeSet::new()), || Ok(Some(Vec::new()))).stand
    };
    assert_eq!(stand(&[("SAFE_PUSH_SOUTH", &missing)]), Stand::Refuse, "ds-940b: SAFE_PUSH_SOUTH at a missing directory skipped the scan");
    assert_eq!(stand(&[("SAFE_PUSH_SOUTH", &no_main)]), Stand::Refuse, "a named south with no main checkout has no deny-list to scan");
    assert_eq!(stand(&[("SAFE_PUSH_DENYLIST", &empty)]), Stand::Refuse);
    assert_eq!(stand(&[("SAFE_PUSH_DENYLIST", &missing)]), Stand::Refuse);
    assert_eq!(stand(&[("NORTH_PRIVATE_KINDS", &missing)]), Stand::Refuse);
    assert_eq!(stand(&[("NORTH_PRIVATE_KINDS", &empty_kinds)]), Stand::Refuse);
    assert_eq!(stand(&[]), Stand::Pass, "a host with no south and no override passes with a warning");
}
