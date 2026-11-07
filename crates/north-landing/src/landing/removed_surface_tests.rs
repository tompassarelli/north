use super::{Kind, Removed, invocation, removed_in};

#[test]
fn a_unit_calling_a_removed_subcommand_under_a_kept_verb_is_named() {
    let removed = Removed { commands: vec!["reclaim".to_owned()], ..Removed::default() };
    let words = invocation(r#"ExecStart = "${northBin}/north lane reclaim";"#, &["${northBin}/"]).unwrap_or_default();
    assert_eq!(removed_in(&words, &removed), vec![(Kind::Command, "reclaim".to_owned())]);
}
