use std::path::Path;
use std::process::Command;

use super::{check, refusal, violations};

#[test]
fn lines_that_turn_the_compiler_cache_off_are_refused_in_every_language_the_repos_use() {
    for line in [
        r#"  CARGO_INCREMENTAL: "1", RUSTC_WRAPPER: "","#,
        r#"RUSTC_WRAPPER="" cargo build --release"#,
        "RUSTC_WRAPPER= cargo build",
        "env -u RUSTC_WRAPPER cargo build --release",
        "unset RUSTC_WRAPPER",
        r#"command.env("RUSTC_WRAPPER", "");"#,
        r#"command.env_remove("RUSTC_WRAPPER");"#,
        "delete env.RUSTC_WRAPPER;",
        r#"rustc-wrapper = """#,
        r#"      RUSTC_WRAPPER = "";"#,
        "CARGO_TARGET_DIR=t CARGO_INCREMENTAL=1 cargo test",
        r#"command.env("CARGO_INCREMENTAL", "1");"#,
    ] {
        assert!(refusal(line).is_some(), "{line}");
    }
}

#[test]
fn lines_that_keep_the_cache_or_only_name_the_variables_pass() {
    for line in [
        r#"  CARGO_BUILD_INCREMENTAL: "true","#,
        "RUSTC_WRAPPER=sccache cargo build",
        r#"      RUSTC_WRAPPER = "${wrapper}";"#,
        "      RUSTC_WRAPPER = sccache;",
        r#"export RUSTC_WRAPPER="$sccache""#,
        "sccache refuses any build under `CARGO_INCREMENTAL` but passes an incremental crate through",
        "CARGO_INCREMENTAL=0 cargo build",
        r#"cache_vars=(RUSTC_WRAPPER SCCACHE_DIR)"#,
    ] {
        assert_eq!(refusal(line), None, "{line}");
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com"]).args(args).output().expect("git");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, file: &str, text: &str, message: &str) -> String {
    std::fs::write(repo.join(file), text).expect("write");
    git(repo, &["add", file]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_landing_that_empties_the_wrapper_is_refused_unless_a_trailer_records_why() {
    let scratch = tempfile::tempdir().expect("scratch");
    let repo = scratch.path();
    git(repo, &["init", "-q"]);
    let base = commit(repo, "library.ts", "export const env = { CARGO_TARGET_DIR: target };\n", "start");
    let head = commit(repo, "library.ts", "export const env = { CARGO_TARGET_DIR: target, CARGO_INCREMENTAL: \"1\", RUSTC_WRAPPER: \"\" };\n", "build with no wrapper");
    assert_eq!(violations(repo, &base, &head).expect("diff").len(), 1);
    assert!(!check(repo, &base, &head));
    let reasoned = commit(repo, "notes.md", "why\n", "record why\n\nBuild-cache-off: the gate's own fixture names the refused shapes");
    assert!(check(repo, &base, &reasoned));
    assert!(check(repo, &head, &reasoned));
}
