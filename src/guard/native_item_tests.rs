use super::native_item::{names_native_item, writes_to_github};
use super::trigger_matrix::exercise;
use crate::platform::sqlite;
use std::path::{Path, PathBuf};
use std::time::Duration;

const NOTE: &str = "the parcel went to the neighbour; authorization for landing and roles across providers and models account 873492 booking Q7R9Z2 contact native@example.test handle @native on 2026-10-26";
const FIRN: &str = "Landed e6a7d3e1 and rebuilt whiterabbit.\n- The journal from the 02:05 sleep shows the dispatcher finished at 02:05:25.455 and Wi-Fi deauthenticated at .456.\nCode landed 2026-10-25 (north 9dc30310, nixos-config d8617e10), deployed to whiterabbit and nexus.";
const WISP: &str = "Box 3 follow-up: the failed Smashcraft Autoland runs 38890506862 (16 Oct) and 39131342098, 39137573264 (18 Oct) cite TSTL errors; smashcraft 25edcd42 added the rule and cost 3.3 s warm.\nBox 3 interim count (2026-10-24 UTC): 19 failed runs, 4 compile-only, run 39262757517 among them.";
const NORTH: &str = "Landed 7ced4545 on origin/main (2026-10-27T11:40:03.832Z): the dispatch test passed in 34.51 s with job 39297631190 and commit fd445065.";
const SMASHCRAFT: &str = "Decision (proxy, 2026-10-27): the gpt-6.1-sol high rise was a mislabel; build 0.0.192 at fa5f7dc039 cut worst frame -5.7%/-6.6% (see #571) for account 873492 and native@example.test.";

fn threads_database(directory: &Path) -> PathBuf {
    let path = directory.join("threads.db");
    let script = format!(
        "CREATE TABLE items(title TEXT, body TEXT); CREATE TABLE events(kind TEXT, data TEXT); INSERT INTO items VALUES ('the orange bicycle beside the gate', 'repaint the garden shed blue tomorrow'); INSERT INTO items VALUES ('gpt-6.1-sol', 'short'); INSERT INTO items VALUES ('Pay rent', 'monthly'); INSERT INTO events VALUES ('note', '{{\"text\":\"{NOTE}\"}}');"
    );
    sqlite::execute(&path, Duration::from_secs(1), &script).unwrap();
    path
}

fn comment(body: &str) -> String {
    format!("gh issue comment 5 --body '{body}'")
}

fn native_refs_and_quoted_item_titles_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let databases = [threads_database(scratch.path())];
    std::fs::write(scratch.path().join("body.md"), "see the orange bicycle beside the gate").unwrap();
    std::fs::write(scratch.path().join("ref.md"), "moved to life#7 for now").unwrap();
    for command in [
        "gh issue comment 5 --body life#1".to_owned(),
        "gh pr edit 3 --body travel#2".to_owned(),
        "gh issue comment 5 --body-file - <<'BODY'\nlife#1\nBODY".to_owned(),
        "gh issue comment 5 --body-file body.md".to_owned(),
        "gh issue comment 5 --body-file ref.md".to_owned(),
        "gh api repos/x/y/issues -F body=@body.md".to_owned(),
        "cd x && gh issue create --title ops#4".to_owned(),
        comment("the orange bicycle beside the gate"),
        comment("THE ORANGE BICYCLE BESIDE THE GATE"),
        comment("the orange\n bicycle beside, the gate"),
        comment("Moved: the orange bicycle beside the gate."),
        comment("accounts#2"),
    ] {
        assert!(names_native_item(&command, scratch.path(), &databases), "{command}");
    }
}

fn adjacent_prose_notes_bodies_and_short_titles_pass() {
    let scratch = tempfile::tempdir().unwrap();
    let databases = [threads_database(scratch.path())];
    for command in [
        "gh issue edit 5 --title 'Harness landing RBAC: authorized roles across providers and models'",
        "gh issue comment 5 --body 'Harness landing RBAC: authorized roles across providers and models'",
        "gh issue comment 5 --body 'the orange bicycle beside the'",
        "gh issue comment 5 --body 'the orange bicycle beside the gates'",
        "gh issue comment 5 --body 'ordinary authorization for landing and prose'",
        "gh issue comment 5 --body 'Tom chose this (2026-10-26)'",
        "gh issue comment 5 --body-file missing.md",
        "gh issue comment 5 --body 'repaint the garden shed blue tomorrow'",
        "gh issue comment 5 --body 'the parcel went to the neighbour'",
        "gh issue comment 5 --body 'account 873492 contact native@example.test handle @native'",
        "gh issue comment 5 --body 'the gpt-6.1-sol high rise'",
        "gh issue comment 5 --body 'Pay rent'",
    ] {
        assert!(!names_native_item(command, scratch.path(), &databases), "{command}");
    }
}

fn without_a_database_only_refs_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let absent = [scratch.path().join("absent.db")];
    assert!(!names_native_item(&comment("the orange bicycle beside the gate"), scratch.path(), &absent));
    assert!(!names_native_item(&comment("the orange bicycle beside the gate"), scratch.path(), &[]));
    assert!(names_native_item("gh issue comment 5 --body life#1", scratch.path(), &[]));
}

fn only_github_writes_are_screened() {
    let scratch = tempfile::tempdir().unwrap();
    let databases = [threads_database(scratch.path())];
    exercise(
        |command| writes_to_github(command) && names_native_item(command, scratch.path(), &databases),
        &["gh issue comment 5 --body life#1", "gh pr edit 3 --body travel#2"],
        &[
            "gh issue view 1",
            "gh issue view life#1",
            "gh issue view 5 --json title,body",
            "gh issue comment 5 --body landed",
            "threads note life#1 progress",
            "rg -n 'life#1' src | head",
            "rg -n 'the orange bicycle beside the gate' docs | head -20",
            "sed -n 100,150p src/lib.rs",
            "head -30 life#1.md",
            "cat notes/the-orange-bicycle-beside-the-gate.md",
        ],
        false,
    );
}

fn public_ids_dates_versions_and_timings_pass_inline_and_from_a_file() {
    let scratch = tempfile::tempdir().unwrap();
    let databases = [threads_database(scratch.path())];
    for (name, text) in [("firn.md", FIRN), ("wisp.md", WISP), ("north.md", NORTH), ("smashcraft.md", SMASHCRAFT)] {
        std::fs::write(scratch.path().join(name), text).unwrap();
        let file = format!("gh issue comment 5 --body-file {name}");
        assert!(!names_native_item(&comment(text), scratch.path(), &databases), "inline {name}");
        assert!(!names_native_item(&file, scratch.path(), &databases), "file {name}");
    }
}

#[test]
fn native_item_decisions_hold() {
    public_ids_dates_versions_and_timings_pass_inline_and_from_a_file();
    only_github_writes_are_screened();
    native_refs_and_quoted_item_titles_are_refused();
    adjacent_prose_notes_bodies_and_short_titles_pass();
    without_a_database_only_refs_are_refused();
}
