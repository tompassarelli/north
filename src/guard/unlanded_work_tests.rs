use super::unlanded_work::stale_lanes_notice;
use crate::platform::archive_tags::{mentions, shell_quote};

#[test]
fn the_notice_counts_only_lanes_older_than_a_day_and_abbreviates_home() {
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("unlanded.txt");
    std::fs::write(&report, "# header\n78h\ta\n24h\tb\n23h\tc\n\n1h\td\n").unwrap();
    let notice = stale_lanes_notice(&report, dir.path().to_str().unwrap()).unwrap();
    assert_eq!(
        notice,
        "2 unlanded worktrees/branches older than 24 h are listed in ~/unlanded.txt (lane-gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\n"
    );
    std::fs::write(&report, "# header\n23h\tc\n").unwrap();
    assert_eq!(stale_lanes_notice(&report, "/nowhere"), None);
    assert_eq!(stale_lanes_notice(&dir.path().join("missing"), "/nowhere"), None);
}

#[test]
fn an_archive_mentions_an_item_only_on_a_refs_line_naming_it() {
    for (message, repo, number, expected) in [
        ("Resume fixture\n\nRefs smashcraft#10", "smashcraft", 10, true),
        ("Resume\n\nrefs: tompassarelli/smashcraft#10, wisp#3", "smashcraft", 10, true),
        ("Resume\n\nREFS wisp#3 smashcraft#10", "smashcraft", 10, true),
        ("Resume\n\nRefs nixos-config#7", "firn", 7, true),
        ("Resume\n\nRefs smashcraft#10", "wisp", 10, false),
        ("Resume\n\nRefs smashcraft#100", "smashcraft", 10, false),
        ("Resume\n\nRefs smashcraft#10", "smashcraft", 1, false),
        ("Mentions smashcraft#10 outside a Refs line", "smashcraft", 10, false),
        ("Refs x/smashcraft#10", "smashcraft", 10, false),
        ("Refs other/smashcraft#10", "smashcraft", 10, false),
        ("Refs smashcraft#", "smashcraft", 10, false),
    ] {
        assert_eq!(mentions(message, repo, number), expected, "{message:?} for {repo}#{number}");
    }
}

#[test]
fn the_restore_command_is_quoted_like_a_shell_word() {
    assert_eq!(shell_quote("archive/feature/x:refs/heads/feature/x"), "archive/feature/x:refs/heads/feature/x");
    assert_eq!(shell_quote("archive/a b:refs/heads/a b"), "'archive/a b:refs/heads/a b'");
    assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
    assert_eq!(shell_quote(""), "''");
}
