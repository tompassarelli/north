use super::{Real, Run, Scratch, git, write};

const DENYLIST: &str = "title = \"safe-push test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''zebra-quokka-7731'''\n";

fn attest(scratch: &Scratch, fixture: &Real, branch: &str, status: &str) -> super::Observed {
    write(&fixture.repo.join("content.txt"), &format!("clean {branch}\n"));
    fixture.commit_all("attested work");
    Run::new(scratch, &fixture.repo, &["--to", branch])
        .env("SAFE_PUSH_SOUTH", scratch.path().join("attest-south"))
        .env("SAFE_PUSH_DENYLIST", scratch.path().join("denylist.toml"))
        .env("SAFE_PUSH_TEST_VISIBILITY", "PUBLIC")
        .env("SAFE_PUSH_TEST_ATTEST_TRACE", scratch.path().join("attest-trace"))
        .env("SAFE_PUSH_TEST_ATTEST_STATUS", status)
        .output()
}

fn attested(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.path().join("attest-trace")).unwrap_or_default()
}

#[test]
fn a_claude_branch_push_to_a_public_autoland_repository_attests_its_tip_and_a_failed_attest_never_undoes_the_push() {
    let scratch = Scratch::new();
    let fixture = Real::new(&scratch, "denylist-attest");
    write(&scratch.path().join("denylist.toml"), DENYLIST);
    std::fs::create_dir_all(scratch.path().join("attest-south")).unwrap();
    write(&fixture.repo.join(".github/workflows/autoland.yml"), "on: [workflow_dispatch]\n");
    write(&fixture.repo.join(".safe-push"), "[landing]\n\tqueue = autoland\n");
    git(&fixture.repo, &["add", ".github/workflows/autoland.yml", ".safe-push"]);
    git(&fixture.repo, &["commit", "-qm", "Autoland workflow"]);
    git(&fixture.repo, &["push", "-q", "origin", "HEAD:main"]);

    let session = "attest-scratch";
    write(&fixture.repo.join("content.txt"), "clean claude/attest-work\n");
    fixture.commit_all("attested work");
    let pushed = Run::new(&scratch, &fixture.repo, &["--to", "claude/attest-work"])
        .env("NORTH_FLEET_SESSION", session)
        .env("SAFE_PUSH_SOUTH", scratch.path().join("attest-south"))
        .env("SAFE_PUSH_DENYLIST", scratch.path().join("denylist.toml"))
        .env("SAFE_PUSH_TEST_VISIBILITY", "PUBLIC")
        .env("SAFE_PUSH_TEST_ATTEST_TRACE", scratch.path().join("attest-trace"))
        .output();
    assert_eq!(pushed.status, Some(0), "{}", pushed.output);
    let repo_name = format!("{}/remote", fixture.remote.parent().unwrap().file_name().unwrap().to_string_lossy());
    let south = scratch.path().join("attest-south");
    assert!(attested(&scratch).lines().any(|line| line == format!("denylist attest --south {} --repo {repo_name} --branch claude/attest-work", south.display())), "a claude/ push did not attest its tip: {}", attested(&scratch));
    let tip = fixture.remote_ref("refs/heads/claude/attest-work").unwrap();
    assert!(pushed.output.contains(&format!("safe-push: pushed {tip} to claude/attest-work")), "{}", pushed.output);
    assert!(!pushed.output.lines().any(|line| line.starts_with("safe-push: landed ")), "a push to a scratch branch was reported as landed: {}", pushed.output);
    let pushes = std::fs::read_to_string(scratch.state().join("safe-push/pushes.tsv")).unwrap_or_default();
    let row: Vec<&str> = pushes.lines().rfind(|line| line.split('\t').nth(5) == Some(session)).unwrap_or_else(|| panic!("a scratch-branch push recorded no push row: {pushes}")).split('\t').collect();
    assert_eq!(row[4], "", "a scratch-branch push recorded a landed sha for fleet: {row:?}");

    let refused = attest(&scratch, &fixture, "claude/attest-refused", "1");
    assert_eq!(refused.status, Some(0), "{}", refused.output);
    assert!(refused.output.contains("the deny-list attester refused claude/attest-refused, so Autoland will not test this tip; reword the deny-listed text and push again"), "{}", refused.output);
    assert!(!refused.output.contains("timer retries"), "a refused attest promised a timer retry: {}", refused.output);

    let deferred = attest(&scratch, &fixture, "claude/attest-deferred", "3");
    assert_eq!(deferred.status, Some(0), "{}", deferred.output);
    assert!(deferred.output.contains("the nexus denylist-attest timer posts the status and dispatches Autoland within about five minutes, with nothing for you to run"), "{}", deferred.output);
    assert!(!deferred.output.contains("failed (exit"), "a deferred attest was reported as a failure: {}", deferred.output);
    assert!(fixture.remote_ref("refs/heads/claude/attest-deferred").is_some(), "a deferred attest undid the push");

    let failed = attest(&scratch, &fixture, "claude/attest-retry", "2");
    assert_eq!(failed.status, Some(0), "{}", failed.output);
    assert!(failed.output.contains("the deny-list attest for claude/attest-retry failed (exit 2); the push stands"), "{}", failed.output);
    assert!(fixture.remote_ref("refs/heads/claude/attest-retry").is_some(), "a failed attest undid the push");

    std::fs::write(scratch.path().join("attest-trace"), "").unwrap();
    write(&fixture.repo.join("content.txt"), "unattested\n");
    fixture.commit_all("unattested work");
    let without_south = Run::new(&scratch, &fixture.repo, &["--to", "claude/attest-no-south"])
        .env("SAFE_PUSH_DENYLIST", scratch.path().join("denylist.toml"))
        .env("SAFE_PUSH_TEST_VISIBILITY", "PUBLIC")
        .env("SAFE_PUSH_TEST_ATTEST_TRACE", scratch.path().join("attest-trace"))
        .output();
    assert_eq!(without_south.status, Some(0), "{}", without_south.output);
    assert!(attested(&scratch).is_empty(), "a push without a south checkout attested: {}", attested(&scratch));
}

#[test]
fn a_branch_landing_refuses_a_private_author_email_before_push() {
    let scratch = Scratch::new();
    let fixture = Real::new(&scratch, "email-to-main");
    let private_email = "someone@private.example";
    git(&fixture.repo, &["commit", "--amend", "--allow-empty", "-qm", "private author for main", &format!("--author=safe-push-test <{private_email}>")]);
    let before = fixture.remote_state();
    let observed = Run::new(&scratch, &fixture.repo, &["--dry-run", "--to", "main"]).output();
    let sha = git(&fixture.repo, &["rev-parse", "--short", "HEAD"]);
    assert_ne!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(&format!("safe-push: commit {sha} author email {private_email} is not safe-push-test@example.invalid")), "{}", observed.output);
    assert!(observed.output.contains("git commit --amend --reset-author (or rebase --exec) before pushing; NOT pushed."), "{}", observed.output);
    assert_eq!(fixture.remote_state(), before, "private author email refusal mutated the remote");
}
