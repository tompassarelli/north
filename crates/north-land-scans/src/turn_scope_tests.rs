use std::path::Path;

use super::*;
use crate::host_evaluate::{self, Evaluate};

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?} in {} exited {}: {}", directory.display(), output.status, String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", "c"]);
    git_in(repo, &["rev-parse", "HEAD"])
}

const ONE: &str = "[[turn]]\ndate = 2026-11-12\n";
const TWO: &str = "[[turn]]\ndate = 2026-11-12\n\n[[turn]]\ndate = 2026-11-13\n";
const BARS: &str = "[[experiment]]\nbar = 1\n";
const MORE_BARS: &str = "[[experiment]]\nbar = 1\n\n[[experiment]]\nbar = 2\n";

#[test]
fn a_lane_outside_a_turns_scope_is_recorded_as_a_would_be_refusal_and_a_batch_is_judged_lane_by_lane_north_1415_and_a_lane_that_breaks_a_host_configuration_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, &[(TURNS, ONE), (EXPERIMENTS, BARS), ("src/a.rs", "a\n")]);

    let inside = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, MORE_BARS)]);
    assert_eq!(check(repo, &base, &inside), Ok(()));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let code = commit(repo, &[(TURNS, TWO), ("src/a.rs", "b\n")]);
    assert!(check(repo, &base, &code).unwrap_err().contains("also changes src/a.rs"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let moved = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, "[[experiment]]\nbar = 9\n")]);
    assert!(check(repo, &base, &moved).unwrap_err().contains("so a bar, window or review day may move"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let rewritten = commit(repo, &[(TURNS, "[[turn]]\ndate = 2026-11-11\n")]);
    assert!(check(repo, &base, &rewritten).unwrap_err().contains("changes or removes a recorded [[turn]]"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let other = commit(repo, &[("src/a.rs", "c\n")]);
    assert_eq!(check(repo, &base, &other), Ok(()));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let turn = commit(repo, &[(TURNS, TWO)]);
    let workflow = commit(repo, &[(".github/workflows/buck2.yml", "on: push\n")]);
    assert_eq!((check(repo, &base, &turn), check(repo, &turn, &workflow)), (Ok(()), Ok(())));
    assert_eq!(judge(repo, &format!("{base}..{workflow}"), "", false), Ok(()));
    assert!(judge(repo, &format!("{base}..{workflow}"), "", true).unwrap_err().contains("also changes .github/workflows/buck2.yml"));
    git_in(repo, &["reset", "-q", "--hard", &other]);
    let state = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    assert_eq!(scan(repo, &format!("{base}..{code}"), state.path(), &mut out), 0);
    assert!(String::from_utf8(out).unwrap().contains("would refuse (shadow)"));
    let log = std::fs::read_to_string(state.path().join("north/gates/turn-scope.jsonl")).unwrap();
    assert!(log.contains("\"would_refuse\":true"), "{log}");

    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let good = "{ imports = [ ../../modules/a.nix ../../modules/b.nix ]; }\n";
    let base = commit(repo, &[("flake.nix", "{}\n"), ("hosts/nexus/configuration.nix", good), ("hosts/gex45/configuration.nix", "{ }\n"), ("modules/a.nix", "{ }\n"), ("modules/b.nix", "{ }\n")]);

    let touched = commit(repo, &[("modules/b.nix", "{ x = 1; }\n")]);
    assert_eq!(host_evaluate::check(repo, &base, &touched, &Imports), Ok(()));
    let both = vec!["gex45".to_owned(), "nexus".to_owned()];
    assert_eq!(host_evaluate::affected(repo, both, &host_evaluate::changed_nix(repo, &base, &touched)), vec!["nexus".to_owned()]);

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let joined = commit(repo, &[("hosts/nexus/configuration.nix", "{ imports = [ ../../modules/a.nix../../modules/b.nix ]; }\n")]);
    let refusal = host_evaluate::check(repo, &base, &joined, &Imports).unwrap_err();
    assert!(refusal.starts_with("nexus cannot evaluate its toplevel"), "{refusal}");
    assert!(!refusal.contains("gex45"), "{refusal}");
}

struct Imports;

impl Evaluate for Imports {
    fn hosts(&self, _: &Path) -> Result<Vec<String>, String> {
        Ok(vec!["gex45".to_owned(), "nexus".to_owned()])
    }

    fn toplevel(&self, directory: &Path, host: &str) -> Result<(), String> {
        let file = Path::new("hosts").join(host).join("configuration.nix");
        let text = std::fs::read_to_string(directory.join(&file)).map_err(|error| error.to_string())?;
        let missing = host_evaluate::imported(directory, &file, &text).into_iter().find(|path| !directory.join(path).exists());
        missing.map_or(Ok(()), |path| Err(format!("error: path '{}' does not exist", path.display())))
    }
}
