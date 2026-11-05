use std::fs;
use std::path::Path;
use std::process::Command;

use super::verdict;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn save(repo: &Path, files: &[(&str, usize)]) -> String {
    for (path, lines) in files {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "rule\n".repeat(*lines)).unwrap();
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "fixture"]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn every_agents_md_at_the_checked_snapshot_is_held_to_100_lines_unless_the_catalog_names_it_a_baseline() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path();
    git(repo, &["init", "-q", "-b", "main"]);
    let valid = save(repo, &[("AGENTS.md", 100), ("nested/AGENTS.md", 100)]);
    assert_eq!(verdict(repo, "HEAD"), (0, "agent-instruction-check: 2 instruction files within limits\n".to_owned()));
    let root_over = save(repo, &[("AGENTS.md", 101)]);
    let (code, report) = verdict(repo, "HEAD");
    assert_eq!(code, 1);
    assert!(report.contains(&format!("AGENTS.md@{root_over}: 101 lines, limit 100")), "{report}");
    assert_eq!(verdict(repo, &valid).0, 0);
    let nested_over = save(repo, &[("AGENTS.md", 100), ("nested/AGENTS.md", 101)]);
    let (code, report) = verdict(repo, "HEAD");
    assert_eq!(code, 1);
    assert!(report.contains(&format!("nested/AGENTS.md@{nested_over}: 101 lines")), "{report}");
    git(repo, &["tag", "-a", "release", "-m", "release"]);
    assert_eq!(verdict(repo, "release").0, 1);
    fs::create_dir_all(repo.join("dotfiles/agents")).unwrap();
    fs::write(
        repo.join("dotfiles/agents/catalog-config.json"),
        r#"{"baselines":[{"owner":{"repo":"nixos-config","path":"dotfiles/agents/AGENTS.md"},"targets":["shared","codex"]}]}"#,
    )
    .unwrap();
    save(repo, &[("nested/AGENTS.md", 100), ("dotfiles/agents/AGENTS.md", 197)]);
    assert_eq!(verdict(repo, "HEAD").0, 0);
    assert_eq!(verdict(repo, "no-such-ref").0, 2);
}
