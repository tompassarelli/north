use super::drafts::{self, Draft, Ground, Outcome};
use std::path::Path;
use std::process::Command;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn draft(name: &str, checkout: &Path, base: &str, file: &str) -> Draft {
    Draft {
        name: name.into(),
        at: 1_000.0,
        item: "north#1240".into(),
        box_text: "a box".into(),
        box_sha256: "0".into(),
        checkout: checkout.to_path_buf(),
        base: base.into(),
        commit: Some("1".into()),
        files: vec![file.into()],
        check: "cargo test".into(),
        outcome: Outcome::Running,
        check_exit: None,
        attempt: None,
        host: "h".into(),
        lane: checkout.to_path_buf(),
        session: None,
        tokens: Some(27_877),
        cost_usd: Some(0.0033),
        failure: None,
    }
}

#[test]
fn a_listed_draft_shows_main_moved_only_when_main_changed_one_of_its_files_and_its_latest_check_result() {
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    git(&checkout, &["init", "-q", "-b", "main"]);
    std::fs::write(checkout.join("moved.rs"), "a").unwrap();
    std::fs::write(checkout.join("kept.rs"), "a").unwrap();
    git(&checkout, &["add", "moved.rs", "kept.rs"]);
    git(&checkout, &["commit", "-q", "-m", "base"]);
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("moved.rs"), "b").unwrap();
    git(&checkout, &["commit", "-q", "-am", "main moves"]);
    git(&checkout, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

    let store = scratch.path().join("dispatch");
    let mut checked = draft("d-moved", &checkout, &base, "moved.rs");
    drafts::append(&store, &checked).unwrap();
    checked.outcome = Outcome::DraftPass;
    checked.check_exit = Some(0);
    drafts::append(&store, &checked).unwrap();
    drafts::append(&store, &draft("d-kept", &checkout, &base, "kept.rs")).unwrap();

    let lines: Vec<String> = drafts::read(&store).iter().map(|draft| drafts::line(draft, &drafts::ground(draft), 1_000.0 + 3_720.0)).collect();
    assert_eq!(
        lines,
        [
            "d-moved north#1240 1h02m draft-pass (exit 0) 27877 tokens $0.0033 main moved under moved.rs: a box",
            "d-kept north#1240 1h02m running 27877 tokens $0.0033 main stands under its files: a box",
        ]
    );
    assert!(matches!(drafts::ground(&draft("d-gone", &checkout, "0000000000000000000000000000000000000000", "kept.rs")), Ground::Unknown(_)));
}
