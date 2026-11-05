use north_core::foundation::shim;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn north() -> &'static str {
    env!("CARGO_BIN_EXE_north")
}

fn linked_twice(scratch: &Path) -> PathBuf {
    let (far, near) = (scratch.join("far"), scratch.join("near"));
    std::fs::create_dir_all(&far).unwrap();
    std::fs::create_dir_all(&near).unwrap();
    symlink(north(), far.join("safe-push")).unwrap();
    symlink(far.join("safe-push"), near.join("safe-push")).unwrap();
    near.join("safe-push")
}

fn real_git() -> String {
    let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn run(command: &mut Command) -> Output {
    command.output().unwrap()
}

#[test]
fn a_safe_push_link_through_two_hops_prints_the_usage_north_safe_push_prints() {
    let scratch = tempfile::tempdir().unwrap();
    let link = linked_twice(scratch.path());
    let through_link = run(Command::new(&link).arg("--help"));
    let through_verb = run(Command::new(north()).args(["safe-push", "--help"]));
    assert!(through_link.status.success(), "{}", String::from_utf8_lossy(&through_link.stderr));
    assert!(!through_link.stdout.is_empty());
    assert_eq!(through_link.stdout, through_verb.stdout);
}

#[test]
fn a_safe_push_link_found_on_path_is_selected_by_its_bare_name() {
    let scratch = tempfile::tempdir().unwrap();
    let link = linked_twice(scratch.path());
    let output = run(Command::new("safe-push").arg("--help").env("PATH", format!("{}:{}", link.parent().unwrap().display(), std::env::var("PATH").unwrap())));
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("safe-push"));
}

#[test]
fn the_entry_runs_its_children_without_codex_home_and_north_itself_does_not_scrub_it() {
    let scratch = tempfile::tempdir().unwrap();
    let link = linked_twice(scratch.path());
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(Command::new(real_git()).current_dir(&repo).args(["init", "-q"]).status().unwrap().success());
    let bin = scratch.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let seen = scratch.path().join("seen");
    shim::write_script(&bin.join("git"), &format!("printf '%s\\n' \"${{CODEX_HOME-unset}}\" >>'{}'\nexec '{}' \"$@\"\n", seen.display(), real_git()));
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let ran = |program: &str, arguments: &[&str]| {
        run(Command::new(program).args(arguments).current_dir(&repo).env("PATH", &path).env("CODEX_HOME", "/pool/account").env("XDG_STATE_HOME", scratch.path().join("state")))
    };
    ran(link.to_str().unwrap(), &["--tag", "entry-test", "--dry-run"]);
    let through_link = std::fs::read_to_string(&seen).unwrap();
    assert!(!through_link.is_empty(), "the entry ran no git");
    assert!(through_link.lines().all(|line| line == "unset"), "{through_link}");
    std::fs::remove_file(&seen).unwrap();
    ran(north(), &["safe-push", "--tag", "entry-test", "--dry-run"]);
    let through_verb = std::fs::read_to_string(&seen).unwrap();
    assert!(through_verb.lines().all(|line| line == "/pool/account"), "{through_verb}");
}
