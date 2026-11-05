use super::check;

fn reasons(base: &str, head: &str) -> Vec<String> {
    check("harness/defaults.toml", Some(base), Some(head)).into_iter().map(|finding| format!("{} {}", finding.key, finding.reason)).collect()
}

#[test]
fn a_changed_value_and_a_new_key_pass() {
    assert!(reasons("[headroom]\nvalue = 2\n", "[headroom]\nvalue = 3\n[spare]\nvalue = 'x'\n").is_empty());
}

#[test]
fn a_removed_key_a_changed_type_and_a_parse_error_are_refused() {
    assert_eq!(reasons("[headroom]\nvalue = 2\nunit = 's'\n", "[headroom]\nvalue = 2.5\n"), ["headroom.unit removed", "headroom.value was Integer, now Float (write the number in the type it had, such as 3.0 for a float)"]);
    assert_eq!(reasons("a = 1\n", "a = \n"), [" does not parse: string values must be quoted, expected literal string"]);
    assert_eq!(check("harness/defaults.toml", Some("a = 1\n"), None)[0].reason, "deleted, but the running north reads it");
}

#[test]
fn every_entry_of_an_array_of_tables_keeps_the_keys_all_earlier_entries_had() {
    let base = "[[models]]\nid = 'a'\nprice = 1.0\nnote = 'x'\n[[models]]\nid = 'b'\nprice = 2.0\n";
    assert!(reasons(base, "[[models]]\nid = 'a'\nprice = 1.5\n[[models]]\nid = 'c'\nprice = 3.0\n[[models]]\nid = 'd'\nprice = 4.0\n").is_empty());
    assert_eq!(reasons(base, "[[models]]\nid = 'a'\nprice = 1.0\n[[models]]\nid = 'b'\nprice = 'free'\n[[models]]\nid = 'c'\n"), ["models[1].price was Float, now String", "models[2].price every earlier entry had it"]);
}

#[test]
fn the_data_on_main_passes_against_itself() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in north_base::build_inputs::RUN_TIME_DATA {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(check(path, Some(&text), Some(&text)).is_empty(), "{path}");
    }
}

#[test]
fn a_prompt_may_change_freely_but_not_disappear() {
    assert!(check("harness/claude/agents/worker.md", Some("---\neffort: medium\n---\n"), Some("anything")).is_empty());
    assert_eq!(check("harness/claude/agents/worker.md", Some("x"), None).len(), 1);
}

#[test]
fn an_array_its_readers_had_entries_in_may_shrink_but_not_empty() {
    assert!(reasons("command = ['bun', 'test']\n", "command = ['bun']\n").is_empty());
    assert_eq!(reasons("command = ['bun', 'test']\n", "command = []\n"), ["command emptied, but its readers had entries"]);
    assert_eq!(reasons("[[m]]\nid = 'a'\n", "m = []\n"), ["m emptied, but its readers had entries"]);
}
