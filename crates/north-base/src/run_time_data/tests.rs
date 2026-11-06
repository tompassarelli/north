use super::check;

fn reasons(base: &str, head: &str) -> Vec<String> {
    check("harness/model-catalog.toml", Some(base), Some(head)).into_iter().map(|finding| format!("{} {}", finding.key, finding.reason)).collect()
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
    for path in crate::build_inputs::RUN_TIME_DATA {
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

const DEFAULTS_PATH: &str = "harness/defaults.toml";

fn defaults_reasons(base: &str, head: &str) -> Vec<String> {
    check(DEFAULTS_PATH, Some(base), Some(head)).into_iter().map(|finding| format!("{} {}", finding.key, finding.reason)).collect()
}

#[test]
fn every_default_on_main_declares_a_range_it_sits_inside() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(DEFAULTS_PATH)).unwrap();
    let document: toml_edit::DocumentMut = text.parse().unwrap();
    let silent: Vec<&str> = document.iter().filter(|(_, entry)| entry.get("value").is_some() && entry.get("allowed").is_none()).map(|(name, _)| name).collect();
    assert!(silent.is_empty(), "no allowed range beside {silent:?}");
    assert!(check(DEFAULTS_PATH, None, Some(&text)).is_empty());
}

#[test]
fn a_value_outside_its_declared_range_is_refused_and_one_inside_passes() {
    let base = "[success_threshold]\nvalue = 80\nallowed = { min = 0, max = 100 }\n";
    assert!(defaults_reasons(base, "[success_threshold]\nvalue = 95\nallowed = { min = 0, max = 100 }\n").is_empty());
    assert_eq!(defaults_reasons(base, "[success_threshold]\nvalue = 180\nallowed = { min = 0, max = 100 }\n"), ["success_threshold value 180 is outside 0..100"]);
    assert_eq!(defaults_reasons(base, "[success_threshold]\nvalue = -1\nallowed = { min = 0, max = 100 }\n"), ["success_threshold value -1 is outside 0..100"]);
}

#[test]
fn a_lane_cannot_widen_the_range_it_is_judged_by() {
    let base = "[success_threshold]\nvalue = 80\nallowed = { min = 0, max = 100 }\n";
    let widened = "[success_threshold]\nvalue = 180\nallowed = { min = 0, max = 200 }\n";
    assert_eq!(defaults_reasons(base, widened), ["success_threshold changed its allowed range, which only the Rust that reads it may widen"]);
}

#[test]
fn a_new_key_declares_its_range_and_sits_inside_it() {
    let base = "[a]\nvalue = 1\nallowed = { min = 0, max = 2 }\n";
    assert_eq!(defaults_reasons(base, &format!("{base}[b]\nvalue = 1\n")), ["b has a value but declares no allowed range or values beside it"]);
    assert_eq!(defaults_reasons(base, &format!("{base}[b]\nvalue = 9\nallowed = {{ min = 0, max = 2 }}\n")), ["b value 9 is outside 0..2"]);
    assert!(defaults_reasons(base, &format!("{base}[b]\nvalue = 1\nallowed = {{ min = 0, max = 2 }}\n")).is_empty());
}

#[test]
fn strings_lists_and_ordered_pairs_are_held_to_their_declared_values() {
    let base = "[route]\nvalue = 'codex'\nallowed = { one_of = ['codex', 'claude'] }\n[tiers]\nvalue = ['codex:sol']\nallowed = { prefix = 'codex:', min_len = 0, max_len = 2 }\n[band]\nvalue = [10, 20]\nallowed = { min = 0, max = 100, min_len = 2, max_len = 2, ascending = true }\n";
    assert!(defaults_reasons(base, base).is_empty());
    let head = base.replace("'codex'\n", "'gemini'\n").replace("['codex:sol']", "['codex:a', 'codex:b', 'codex:c']").replace("[10, 20]", "[30, 20]");
    assert_eq!(
        defaults_reasons(base, &head),
        ["band value entries must not decrease", "route value \"gemini\" is not one of [\"codex\", \"claude\"]", "tiers value holds 3 entries, allowed 0..2"]
    );
    assert_eq!(defaults_reasons(base, &base.replace("['codex:sol']", "['claude:x']")), ["tiers value \"claude:x\" does not start with \"codex:\""]);
}

#[test]
fn a_constraint_no_reader_checks_is_refused() {
    let head = "[a]\nvalue = 1\nallowed = { minimum = 0 }\n";
    assert_eq!(defaults_reasons(head, head), ["a value allowed names minimum, which no reader of it checks"]);
}

fn reject_odd_numbers(text: &str) -> Result<(), String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|_| "unreadable".to_owned())?;
    let count = document.get("count").and_then(|item| item.as_integer()).ok_or("count is not a whole number")?;
    (count % 2 == 0).then_some(()).ok_or_else(|| format!("count {count} is odd"))
}

static TEST_READERS: [super::Consumer; 1] = [super::Consumer { path: "harness/test-reader.toml", validate: reject_odd_numbers }];

#[test]
fn a_value_the_installed_reader_rejects_is_refused_though_it_parses_and_keeps_its_type() {
    super::install(&TEST_READERS);
    let reasons = |base: Option<&str>, head: &str| check("harness/test-reader.toml", base, Some(head)).into_iter().map(|finding| finding.reason).collect::<Vec<_>>();
    assert!(reasons(Some("count = 2\n"), "count = 4\n").is_empty());
    assert_eq!(reasons(Some("count = 2\n"), "count = 3\n"), ["refused by the reader that loads it: count 3 is odd"]);
    assert_eq!(reasons(None, "count = 3\n"), ["refused by the reader that loads it: count 3 is odd"]);
    assert!(check("harness/other.toml", Some("count = 2\n"), Some("count = 3\n")).is_empty());
}

fn git(directory: &std::path::Path, arguments: &[&str]) {
    let status = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).current_dir(directory).args(arguments).status().unwrap();
    assert!(status.success(), "git {arguments:?}");
}

fn lane(base: &str, head: &str) -> (tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("harness")).unwrap();
    git(directory.path(), &["init", "-q"]);
    std::fs::write(directory.path().join(DEFAULTS_PATH), base).unwrap();
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-q", "-m", "base"]);
    std::fs::write(directory.path().join(DEFAULTS_PATH), head).unwrap();
    git(directory.path(), &["commit", "-q", "-am", "head"]);
    (directory, "HEAD~1..HEAD".to_owned())
}

#[test]
fn the_scan_refuses_a_table_only_lane_that_sets_an_out_of_range_value_and_passes_a_valid_one() {
    let base = "[success_threshold]\nvalue = 80\nallowed = { min = 0, max = 100 }\n";
    let (refused, range) = lane(base, "[success_threshold]\nvalue = 180\nallowed = { min = 0, max = 100 }\n");
    assert_eq!(super::run(refused.path(), &range), 1);
    let (accepted, range) = lane(base, "[success_threshold]\nvalue = 90\nallowed = { min = 0, max = 100 }\n");
    assert_eq!(super::run(accepted.path(), &range), 0);
}

#[test]
fn a_pair_of_limits_is_held_to_its_order_even_when_each_sits_inside_its_own_range() {
    let base = "[low]\nvalue = 4\nallowed = { min = 1, max = 256, at_most = 'high' }\n[high]\nvalue = 12\nallowed = { min = 1, max = 256 }\n";
    assert!(defaults_reasons(base, base).is_empty());
    assert_eq!(defaults_reasons(base, &base.replace("value = 12", "value = 3")), ["low value must be at most high (3)"]);
    assert_eq!(defaults_reasons(base, &base.replace("value = 4", "value = 40")), ["low value must be at most high (12)"]);
}

#[test]
fn a_list_may_empty_when_its_declared_range_allows_it() {
    let base = "[tiers]\nvalue = ['codex:a']\nallowed = { prefix = 'codex:', min_len = 0 }\n";
    assert!(defaults_reasons(base, &base.replace("['codex:a']", "[]")).is_empty());
    let kept = "[owners]\nvalue = ['a']\nallowed = { min_len = 1 }\n";
    assert_eq!(defaults_reasons(kept, &kept.replace("['a']", "[]")), ["owners value holds 0 entries, allowed 1..any"]);
}
