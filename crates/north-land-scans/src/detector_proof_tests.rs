use std::path::Path;

use super::*;

const BODY: &str = "Done when:\n- [ ] **Stalled plan box.** north#1 sat for 30 h. Done when: the box moves (detector plan:north-1-a)\n";
const KEPT: &str = "{\"arm\":\"base\",\"repeats\":true,\"detector\":\"pattern\"}\n{\"arm\":\"base\",\"repeats\":true,\"detector\":\"pattern\"}\n{\"arm\":\"lane\",\"repeats\":false,\"detector\":\"pattern\"}\n{\"arm\":\"lane\",\"repeats\":false,\"detector\":\"pattern\"}\n";
const INCONCLUSIVE: &str = "{\"arm\":\"base\",\"repeats\":true,\"detector\":\"judge\"}\n{\"arm\":\"base\",\"repeats\":true,\"detector\":\"judge\"}\n{\"arm\":\"lane\",\"repeats\":false,\"detector\":\"judge\"}\n{\"arm\":\"lane\",\"repeats\":false,\"detector\":\"judge\"}\n";

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
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", message]);
    git_in(repo, &["rev-parse", "HEAD"])
}

fn judged(repo: &Path, state: &Path, base: &str, head: &str) -> (u8, String) {
    let mut out = Vec::new();
    let status = scan(repo, &format!("{base}..{head}"), state, &|_| Ok(BODY.to_owned()), &mut out);
    (status, String::from_utf8(out).unwrap())
}

#[test]
fn a_harness_text_lane_ticking_a_detector_box_lands_only_with_a_case_whose_replay_reads_kept() {
    let root = tempfile::tempdir().unwrap();
    let (repo, state) = (root.path().join("repo"), root.path().join("state"));
    std::fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(REGISTRY, "[gate.detector-proof]\nmode = \"enforce\"\n"), ("harness/skills/workers/SKILL.md", "old\n")], "base");
    let ticks = "Restaff a stalled plan box\n\nItem: tompassarelli/north#1\nBox: Stalled plan box.";
    let text = ("harness/skills/workers/SKILL.md", "restaff a box stalled 24 h\n");

    let bare = commit(&repo, &[text], ticks);
    let (status, said) = judged(&repo, &state, &base, &bare);
    assert_eq!((status, said.contains("refused") && said.contains("adds no case")), (1, true), "{said}");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let unsure = commit(&repo, &[text, ("harness/cases/stall.toml", "kind = \"bad\"\n"), ("harness/cases/stall.proof.jsonl", INCONCLUSIVE)], ticks);
    let (status, said) = judged(&repo, &state, &base, &unsure);
    assert_eq!((status, said.contains("inconclusive")), (1, true), "{said}");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let proven = commit(&repo, &[text, ("harness/cases/stall.toml", "kind = \"bad\"\n"), ("harness/cases/stall.proof.jsonl", KEPT)], ticks);
    let (status, said) = judged(&repo, &state, &base, &proven);
    assert_eq!(status, 0, "{said}");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let shadow = commit(&repo, &[(REGISTRY, "[gate.detector-proof]\nmode = \"shadow\"\n")], "shadow");
    let bare = commit(&repo, &[text], ticks);
    let (status, said) = judged(&repo, &state, &shadow, &bare);
    assert_eq!((status, said.contains("would refuse")), (0, true), "{said}");
    let log = std::fs::read_to_string(state.join("north/gates/detector-proof.jsonl")).unwrap();
    assert!(log.lines().last().unwrap().contains("\"would_refuse\":true"), "{log}");
}
