use super::{BARE, Kind, Removed, matching, sources};
use std::path::{Path, PathBuf};

#[test]
fn a_goal_oracle_or_south_module_calling_a_removed_verb_is_named() {
    let removed = Removed { commands: vec!["provenance".to_owned(), "credentials".to_owned()], ..Removed::default() };
    let oracle = matching(&removed, "wisp origin/main", "goals.toml:27", r#"oracle = "north provenance debt --due | wc -l""#, &BARE);
    assert_eq!(oracle.iter().map(|found| (found.kind, found.entry.as_str())).collect::<Vec<_>>(), [(Kind::Command, "provenance")]);
    let unit = matching(&removed, "south origin/main", "modules/north-credentials.nix:15", r#"ExecStart = jobRun "x" "${northBin}/north credentials renew";"#, &["${northBin}/"]);
    assert_eq!(unit.len(), 1);
    let [north, _, south, ..] = &sources(Path::new("tree"), "HEAD", PathBuf::new(), PathBuf::new());
    assert!(south.pathspecs.contains(&"modules"));
    assert_eq!((north.rev.as_str(), north.pathspecs), ("HEAD", &["bin", "harness", ":(exclude)*.md"][..]));
    let pace = Removed { commands: vec!["pace".to_owned()], ..Removed::default() };
    let script = matching(&pace, "north HEAD", "bin/pace-control:3", r#"exec "${NORTH_PACE_BIN:-north}" pace sample "$@""#, north.prefixes);
    assert_eq!(script.iter().map(|found| found.entry.as_str()).collect::<Vec<_>>(), ["pace"]);
    let moved = Removed { live: vec!["fleet pace".to_owned()], ..pace };
    assert!(matching(&moved, "north HEAD", "bin/pace-control:3", r#"exec "${NORTH_PACE_BIN:-north}" fleet pace sample "$@""#, north.prefixes).is_empty());
}
