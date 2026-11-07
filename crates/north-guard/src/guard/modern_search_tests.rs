#![cfg(test)]

use super::modern_search::{Decision, decide};

// KEEP(bug): 8 of 30 sampled modern-search refusals were false: a redirect word or an unexpanded glob made `grep -r` on plain files look like a tree search; north#706
#[test]
fn recursive_grep_on_files_with_redirects_or_globs_is_left_alone_and_a_tree_search_still_rewrites() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_str().unwrap();
    std::fs::write(dir.path().join("config.kdl"), "spawn").unwrap();
    std::fs::write(dir.path().join("a.db"), "x").unwrap();
    std::fs::write(dir.path().join("b.db"), "x").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    for command in [
        "grep -rn spawn config.kdl 2>/dev/null | head -5",
        "grep -rn spawn config.kdl > out.txt",
        "grep -rn spawn *.db 2>/dev/null",
        "ls *.db 2>/dev/null | head; grep -rn spawn ./*.db",
    ] {
        assert_eq!(decide(command, base, "/home/x"), None, "{command}");
    }
    for command in ["grep -rn spawn src 2>/dev/null", "grep -rn spawn *", "grep -rn '>' src", "grep -rn spawn src/*.db"] {
        assert!(decide(command, base, "/home/x").is_some(), "{command}");
    }
    assert!(matches!(decide("grep -rn spawn src 2>/dev/null", base, "/home/x"), Some(Decision::Rewrite(rewritten)) if rewritten == "rg -n spawn src 2>/dev/null"));
}
