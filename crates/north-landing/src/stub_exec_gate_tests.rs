use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use super::{ALLOWLIST, Allowlist, check, problems, violations};

const STUB: &str = concat!("fs::set_permissions(&p, fs::Permissions::from_", "mode(0o755)).unwrap();\n");
const COPY: &str = concat!("std::fs::co", "py(env!(\"CARGO_BIN_EXE_north\"), &p);\n");
const HELPER: &str = concat!("fn execu", "table(path: &Path, body: &str) {}\n");
const CLEAN: &str = "shim::write_script(&p, body);\n";

fn listed(entries: &[(&str, &str)]) -> Allowlist {
    entries.iter().map(|(path, reason)| ((*path).to_owned(), (*reason).to_owned())).collect()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com"]).args(args).output().expect("git");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, Option<&str>)]) -> String {
    for (file, text) in files {
        let path = repo.join(file);
        match text {
            Some(text) => {
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(&path, text).expect("write");
            }
            None => std::fs::remove_file(&path).expect("remove"),
        }
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "change"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn allowlist_json(entries: &[(&str, &str)]) -> String {
    serde_json::to_string(&listed(entries)).expect("json")
}

#[test]
fn a_site_that_makes_a_file_executable_is_flagged_unless_it_is_allowed() {
    let files: BTreeMap<String, String> = [("tests/new_test.rs", STUB), ("crates/install.rs", STUB), ("tests/copy.rs", COPY), ("tests/helper.rs", HELPER), ("tests/clean.rs", CLEAN), ("tests/migrated.rs", CLEAN)]
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect();
    let allowed = listed(&[("crates/install.rs", "production"), ("tests/migrated.rs", "a wrapper"), ("tests/gone.rs", "a wrapper")]);
    let found = problems(&files, &allowed);
    let paths: Vec<&str> = found.iter().map(|line| line.split(": ").next().expect("path")).collect();
    assert_eq!(paths, ["tests/copy.rs", "tests/helper.rs", "tests/new_test.rs", ALLOWLIST, ALLOWLIST], "{found:?}");
    assert!(found[3].contains("tests/gone.rs (a wrapper) no longer makes a file executable") && found[4].contains("tests/migrated.rs"), "{found:?}");
}

#[test]
fn a_landing_that_adds_a_stub_outside_the_shim_is_refused_against_the_allowlist_it_carries() {
    let scratch = tempfile::tempdir().expect("scratch");
    let repo = scratch.path();
    git(repo, &["init", "-q"]);
    let base = commit(repo, &[(ALLOWLIST, Some(&allowlist_json(&[("crates/install.rs", "production")]))), ("crates/install.rs", Some(STUB)), ("tests/old.rs", Some(CLEAN))]);
    let stub = commit(repo, &[("tests/stub.rs", Some(STUB)), ("docs/note.md", Some(STUB))]);
    assert_eq!(violations(repo, &base, &base, &stub).expect("diff").len(), 1);
    assert!(!check(repo, &base, &base, &stub));
    let allowed = commit(repo, &[(ALLOWLIST, Some(&allowlist_json(&[("crates/install.rs", "production"), ("tests/stub.rs", "production")])))]);
    assert!(check(repo, &base, &base, &allowed), "the landing's own allowlist entry admits it");
    let shimmed = commit(repo, &[("tests/old.rs", Some("shim::copy_executable(&a, &b);\n"))]);
    assert!(check(repo, &allowed, &allowed, &shimmed), "a file that copies through the shim passes");
}

#[test]
fn a_landing_that_retires_an_allowed_site_must_drop_its_allowance() {
    let scratch = tempfile::tempdir().expect("scratch");
    let repo = scratch.path();
    git(repo, &["init", "-q"]);
    let base = commit(repo, &[(ALLOWLIST, Some(&allowlist_json(&[("tests/wrapper.rs", "a wrapper"), ("tests/other.rs", "a wrapper")]))), ("tests/wrapper.rs", Some(HELPER)), ("tests/other.rs", Some(HELPER))]);
    let migrated = commit(repo, &[("tests/wrapper.rs", Some(CLEAN))]);
    let found = violations(repo, &base, &base, &migrated).expect("diff");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with(&format!("{ALLOWLIST}: tests/wrapper.rs (a wrapper)")), "{found:?}");
    let deleted = commit(repo, &[("tests/wrapper.rs", None)]);
    assert_eq!(violations(repo, &base, &base, &deleted).expect("diff").len(), 1, "a deleted allowed file needs its entry dropped");
    let dropped = commit(repo, &[(ALLOWLIST, Some(&allowlist_json(&[("tests/other.rs", "a wrapper")])))]);
    assert!(check(repo, &base, &base, &dropped));
}

#[test]
fn a_repository_without_the_allowlist_is_not_judged() {
    let scratch = tempfile::tempdir().expect("scratch");
    let repo = scratch.path();
    git(repo, &["init", "-q"]);
    let base = commit(repo, &[("README.md", Some("x\n"))]);
    let head = commit(repo, &[("tests/stub.rs", Some(STUB))]);
    assert_eq!(violations(repo, &base, &base, &head).expect("diff"), Vec::<String>::new());
}

#[test]
fn a_lane_branched_before_the_allowlist_is_judged_against_the_destinations_allowlist() {
    let scratch = tempfile::tempdir().expect("scratch");
    let repo = scratch.path();
    git(repo, &["init", "-q"]);
    let base = commit(repo, &[("README.md", Some("x\n"))]);
    let lane = commit(repo, &[("tests/stub.rs", Some(STUB))]);
    git(repo, &["checkout", "-q", "-b", "destination", &base]);
    let destination = commit(repo, &[(ALLOWLIST, Some(&allowlist_json(&[("crates/install.rs", "production")]))), ("crates/install.rs", Some(STUB))]);
    let found = violations(repo, &destination, &base, &lane).expect("diff");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("tests/stub.rs: "), "{found:?}");
    let clean = commit(repo, &[]);
    git(repo, &["checkout", "-q", &lane]);
    let shimmed = commit(repo, &[("tests/stub.rs", Some(CLEAN))]);
    assert!(check(repo, &clean, &base, &shimmed), "a predating lane with no stub passes without tripping the destination's other entries");
}
