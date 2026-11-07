use super::{BARE, Kind, Removed, matching, sources};
use std::path::PathBuf;

#[test]
fn a_goal_oracle_or_south_module_calling_a_removed_verb_is_named() {
    let removed = Removed { commands: vec!["provenance".to_owned(), "credentials".to_owned()], ..Removed::default() };
    let oracle = matching(&removed, "wisp origin/main", "goals.toml:27", r#"oracle = "north provenance debt --due | wc -l""#, &BARE);
    assert_eq!(oracle.iter().map(|found| (found.kind, found.entry.as_str())).collect::<Vec<_>>(), [(Kind::Command, "provenance")]);
    let unit = matching(&removed, "south origin/main", "modules/north-credentials.nix:15", r#"ExecStart = jobRun "x" "${northBin}/north credentials renew";"#, &["${northBin}/"]);
    assert_eq!(unit.len(), 1);
    let south = &sources(PathBuf::new(), PathBuf::new())[1];
    assert!(south.pathspecs.contains(&"modules"));
}
