use super::check;

fn reasons(base: &str, head: &str) -> Vec<String> {
    check("harness/model-catalog.toml", Some(base), Some(head)).into_iter().map(|finding| format!("{} {}", finding.key, finding.reason)).collect()
}

#[test]
fn every_entry_of_an_array_of_tables_keeps_the_keys_all_earlier_entries_had() {
    let base = "[[models]]\nid = 'a'\nprice = 1.0\nnote = 'x'\n[[models]]\nid = 'b'\nprice = 2.0\n";
    assert!(reasons(base, "[[models]]\nid = 'a'\nprice = 1.5\n[[models]]\nid = 'c'\nprice = 3.0\n[[models]]\nid = 'd'\nprice = 4.0\n").is_empty());
    assert_eq!(reasons(base, "[[models]]\nid = 'a'\nprice = 1.0\n[[models]]\nid = 'b'\nprice = 'free'\n[[models]]\nid = 'c'\n"), ["models[1].price was Float, now String", "models[2].price every earlier entry had it"]);
}

const DEFAULTS_PATH: &str = "harness/defaults.toml";

#[test]
fn every_default_on_main_declares_a_range_it_sits_inside() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(DEFAULTS_PATH)).unwrap();
    let document: toml_edit::DocumentMut = text.parse().unwrap();
    let silent: Vec<&str> = document.iter().filter(|(_, entry)| entry.get("value").is_some() && entry.get("allowed").is_none()).map(|(name, _)| name).collect();
    assert!(silent.is_empty(), "no allowed range beside {silent:?}");
    assert!(check(DEFAULTS_PATH, None, Some(&text)).is_empty());
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
