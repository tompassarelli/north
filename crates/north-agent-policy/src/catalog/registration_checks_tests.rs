use north_foundation::test_inputs::git::git_in as git;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_pinned_beside_checkout_answers_its_main_without_a_fetch_into_its_read_only_git_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (beside, elsewhere) = (dir.path().join("beside"), dir.path().join("elsewhere"));
    let checkout = beside.join("nixos-config");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    git(&checkout, &["init", "-q", "-b", "main"]);
    git(&checkout, &["remote", "add", "origin", "/nonexistent/origin"]);
    git(&checkout, &["commit", "-q", "--allow-empty", "-m", "pinned"]);
    let pinned = git(&checkout, &["rev-parse", "HEAD"]);
    let git_dir = checkout.join(".git");
    let writable = std::fs::metadata(&git_dir).unwrap().permissions();
    std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let answered = super::pinned_beside(&checkout, Some(&beside)).map(|answer| answer.unwrap());
    let outside = super::pinned_beside(&checkout, Some(&elsewhere)).is_none();
    std::fs::set_permissions(&git_dir, writable).unwrap();
    assert_eq!(answered, Some(pinned), "on 2026-11-15 south's after-landing check-owners fetched origin/main into a beside checkout whose git directory the check's jail binds read-only, and failed 'cannot lock ref refs/north/check-owners/...' three times, leaving south main unverified (north#1578)");
    assert!(outside, "a checkout outside the beside directory was read as pinned instead of fetched");
}
