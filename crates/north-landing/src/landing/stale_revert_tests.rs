use super::*;

const HOUR: i64 = 60 * 60;
const START: i64 = 1_790_000_000;

fn git_ok(root: &Path, args: &[&str]) -> String {
    git(root, args).unwrap_or_else(|error| panic!("{error}")).trim().to_owned()
}

fn commit_staged(root: &Path, message: &str, at: i64) -> String {
    git_ok(root, &["add", "-A"]);
    let date = format!("@{at} +0000");
    let status = Command::new("git")
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args(["-c", "commit.gpgsign=false", "commit", "-qm", message])
        .status()
        .unwrap();
    assert!(status.success());
    git_ok(root, &["rev-parse", "HEAD"])
}

fn commit_at(root: &Path, path: &str, text: &str, message: &str, at: i64) -> String {
    std::fs::write(root.join(path), text).unwrap();
    commit_staged(root, message, at)
}

fn fixture() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.name", "fixture"]);
    git_ok(root, &["config", "user.email", "fixture@example.invalid"]);
    commit_at(root, "app.txt", "alpha\nbeta\ngamma\n", "base", START);
    scratch
}

fn shas(found: &[Reverted]) -> Vec<&str> {
    found.iter().map(|reverted| reverted.commit.as_str()).collect()
}

#[test]
fn a_lane_based_before_a_commit_that_rewrites_its_lines_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x", START + HOUR);
    commit_at(root, "other.txt", "unrelated\n", "main moves on", START + 72 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = commit_at(root, "app.txt", "alpha\nbeta from lane\ngamma\n", "lane", START + 2 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    commit_at(root, "app.txt", "alpha\nbeta from lane\ngamma\n", "lane rebased with its side taken", START + 73 * HOUR);
    let found = reverted(root, "main", "landed", &lane).unwrap();
    assert_eq!(shas(&found), [x.as_str()]);
    assert_eq!(found[0].files.keys().collect::<Vec<_>>(), ["app.txt"]);
}

#[test]
fn a_lane_on_main_that_copied_a_file_from_an_older_ref_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    let old = git_ok(root, &["rev-parse", "HEAD"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "x", START + 100 * HOUR);
    let y = commit_at(root, "app.txt", "alpha changed by y\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "y", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    git_ok(root, &["checkout", &old, "--", "app.txt"]);
    commit_at(root, "notes.txt", "lane work\n", "lane copies app.txt from an older ref", START + 102 * HOUR);
    let found = reverted(root, "main", "lane", "lane").unwrap();
    let mut expected = [x.as_str(), y.as_str()];
    expected.sort_unstable();
    assert_eq!(shas(&found), expected);
    let message = refusal(&found);
    assert!(message.contains("app.txt (3 lines)"), "{message}");
    assert!(message.contains("rebase; don't copy whole files from another ref; squash only after rebasing"), "{message}");
}

#[test]
fn a_soft_reset_squash_onto_moved_main_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "notes.txt", "lane work\n", "lane", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "main"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    git_ok(root, &["reset", "-q", "--soft", "main"]);
    commit_at(root, "notes.txt", "lane work squashed\n", "lane squashed without a rebase", START + 102 * HOUR);
    assert_eq!(shas(&reverted(root, "main", "lane", "lane").unwrap()), [x.as_str()]);
}

#[test]
fn a_squash_after_a_rebase_lands() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "notes.txt", "lane work\n", "lane", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "main"]);
    commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    git_ok(root, &["rebase", "-q", "main"]);
    git_ok(root, &["reset", "-q", "--soft", "main"]);
    commit_at(root, "notes.txt", "lane work squashed\n", "lane squashed after a rebase", START + 102 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn a_reverts_trailer_naming_the_commit_lets_the_lane_land() {
    let scratch = fixture();
    let root = scratch.path();
    let x = commit_at(root, "app.txt", "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "x", START + HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "app.txt", "alpha\nbeta\ngamma\n", &format!("drop x's lines\n\nReverts: {}", &x[..12]), START + 2 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn a_rebased_lane_that_edits_its_own_and_older_lines_lands() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "app.txt", "top\nalpha\nbeta\ngamma\n", "top", START + HOUR);
    git_ok(root, &["branch", "lane"]);
    commit_at(root, "app.txt", "top\nalpha\nbeta\ngamma\ndelta from x\n", "x", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    commit_at(root, "app.txt", "lane first\ntop\nalpha\nbeta\ngamma\n", "lane first", START + 100 * HOUR);
    git_ok(root, &["rebase", "-q", "main"]);
    commit_at(root, "app.txt", "lane first edited\ntop\ndelta from x\n", "lane second", START + 101 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn a_removed_hunk_header_inside_a_file_is_not_read_as_a_path() {
    let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -2,3 +1,0 @@\n--- a/other\n-x\n-y\n@@ -9 +7 @@\n-z\n+w\n";
    let found = changes(patch);
    assert_eq!(found.len(), 1);
    let spans: Vec<(usize, usize)> = found[0].hunks.iter().map(|hunk| (hunk.start, hunk.count)).collect();
    assert_eq!((found[0].path.as_str(), spans.as_slice()), ("f", [(2, 3), (9, 1)].as_slice()));
    assert_eq!(found[0].removed, ["-- a/other", "x", "y", "z"].map(str::to_owned).into());
}

#[test]
fn a_lane_that_moves_a_recent_commits_lines_to_another_file_lands() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "app.txt", "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "x", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    std::fs::write(root.join("moved.txt"), "delta\nepsilon\nzeta\n").unwrap();
    commit_at(root, "app.txt", "alpha\nbeta\ngamma\n", "move x's lines to moved.txt", START + 101 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn a_regenerated_file_marked_linguist_generated_lands() {
    let scratch = fixture();
    let root = scratch.path();
    std::fs::write(root.join(".gitattributes"), "bundle.js linguist-generated=true\n").unwrap();
    commit_at(root, "bundle.js", "one\ntwo\nthree\n", "an older bundle", START);
    commit_at(root, "bundle.js", "uno\ndos\ntres\n", "x rebuilds the bundle", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "bundle.js", "one\ntwo\nthree\n", "lane rebuilds the bundle from older inputs", START + 101 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}


#[test]
fn a_path_with_a_space_is_blamed_not_refused_as_unreadable() {
    let scratch = fixture();
    let root = scratch.path();
    let x = commit_at(root, "my app.txt", "one\ntwo\nthree\n", "x", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "my app.txt", "", "lane empties it", START + 101 * HOUR);
    assert_eq!(shas(&reverted(root, "main", "lane", "lane").unwrap()), [x.as_str()]);
}

#[test]
fn a_lane_stacked_on_a_lane_that_landed_first_lands() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "app.txt", "a1\na2\na3\na4\na5\na6\n", "six lines", START);
    git_ok(root, &["checkout", "-q", "-b", "first"]);
    let first = commit_at(root, "app.txt", "a1\na2\na3\na4 from first\na5\na6\n", "first", START + HOUR);
    git_ok(root, &["checkout", "-q", "-b", "stacked"]);
    let stacked = commit_at(root, "app.txt", "a1\na2\na3\na4 from stacked\na5\na6\n", "stacked", START + 2 * HOUR);
    git_ok(root, &["checkout", "-q", "main"]);
    commit_at(root, "app.txt", "a1\na2 from main\na3\na4\na5\na6\n", "main edits a nearby line", START + 3 * HOUR);
    git_ok(root, &["cherry-pick", &first]);
    git_ok(root, &["checkout", "-q", "stacked"]);
    git_ok(root, &["rebase", "-q", "main"]);
    assert_eq!(reverted(root, "main", "stacked", &stacked).unwrap(), []);
}

#[test]
fn a_lane_that_rewrites_a_recent_commits_lines_lands() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "app.rs", "let cfg = load_config();\nvalidate(&cfg);\napply(cfg);\n", "x", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(root, "app.rs", "let cfg = load_config()?;\nvalidate(&cfg)?;\napply(cfg)?;\n", "fix forward", START + 101 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn lines_a_lane_adds_away_from_a_recent_commits_change_do_not_restore_it() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "app.rs", "fn parse() {\nif s.is_empty() {\nreturn None;\n}\ns.parse().ok()\n}\n\nfn other() {}\n", "parse", START);
    commit_at(root, "app.rs", "fn parse() {\nparse_simply(s)\n}\n\nfn other() {}\n", "x simplifies parse", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    commit_at(
        root,
        "app.rs",
        "fn parse() {\nparse_simply(s).map(Some)\n}\n\nfn other() {}\n\nfn guard() {\nif s.is_empty() {\nreturn None;\n}\n}\n",
        "lane edits x's line and adds a guard",
        START + 101 * HOUR,
    );
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

fn remove_at(root: &Path, path: &str, message: &str, at: i64) -> String {
    std::fs::remove_file(root.join(path)).unwrap();
    commit_staged(root, message, at)
}

#[test]
fn a_lane_that_deletes_a_whole_file_its_base_held_lands() {
    let scratch = fixture();
    let root = scratch.path();
    commit_at(root, "port.txt", "one\ntwo\nthree\nfour\n", "x ports a file", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "-b", "lane"]);
    remove_at(root, "port.txt", "lane retires the port", START + 101 * HOUR);
    assert_eq!(reverted(root, "main", "lane", "lane").unwrap(), []);
}

#[test]
fn a_stale_squash_that_deletes_a_file_main_added_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    let x = commit_at(root, "port.txt", "one\ntwo\nthree\nfour\n", "x adds a file", START + 100 * HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = commit_at(root, "notes.txt", "lane work\n", "lane", START + 101 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    remove_at(root, "port.txt", "squash that never saw the file", START + 102 * HOUR);
    let found = reverted(root, "main", "landed", &lane).unwrap();
    assert_eq!(shas(&found), [x.as_str()]);
    assert_eq!(found[0].files.keys().collect::<Vec<_>>(), ["port.txt"]);
}

fn move_file(root: &Path, from: &str, to: &str, message: &str, at: i64) -> String {
    git_ok(root, &["mv", from, to]);
    commit_staged(root, message, at)
}

#[test]
fn a_pure_file_move_rebased_over_a_recent_edit_of_that_file_lands() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x edits the file", START + HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = move_file(root, "app.txt", "moved.txt", "lane moves the file", START + 2 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    move_file(root, "app.txt", "moved.txt", "lane rebased onto main", START + 3 * HOUR);
    assert_eq!(reverted(root, "main", "landed", &lane).unwrap(), []);
}

#[test]
fn a_file_move_that_also_rewrites_a_recent_commits_lines_is_refused() {
    let scratch = fixture();
    let root = scratch.path();
    git_ok(root, &["branch", "lane"]);
    let x = commit_at(root, "app.txt", "alpha\nbeta from x\ngamma\n", "x edits the file", START + HOUR);
    git_ok(root, &["checkout", "-q", "lane"]);
    let lane = move_file(root, "app.txt", "moved.txt", "lane moves the file", START + 2 * HOUR);
    git_ok(root, &["checkout", "-q", "-B", "landed", "main"]);
    git_ok(root, &["mv", "app.txt", "moved.txt"]);
    std::fs::write(root.join("moved.txt"), "alpha\nbeta\ngamma\n").unwrap();
    commit_staged(root, "move that takes the lane's side", START + 3 * HOUR);
    let found = reverted(root, "main", "landed", &lane).unwrap();
    assert_eq!(shas(&found), [x.as_str()]);
    assert_eq!(found[0].files.keys().collect::<Vec<_>>(), ["app.txt"]);
}
