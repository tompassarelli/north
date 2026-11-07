use super::{Call, calls};

fn call(line: usize, words: &[&str]) -> Call {
    Call { line, words: words.iter().map(|word| (*word).to_owned()).collect() }
}

#[test]
fn a_rust_call_naming_the_verb_after_its_row_was_deleted_is_found_in_each_form_north_builds_it() {
    let forward = "let mut words = vec![\"env\".to_owned()];\nwords.extend([\"north\".to_owned(), \"laptop\".to_owned()]);\nwords.extend(argv.iter().cloned());\n";
    assert_eq!(calls(forward), [call(2, &["laptop"])]);
    assert_eq!(calls("Command::new(north_binary()).args([\"capacity\", \"offload\", \"warm\"]).status()"), [call(1, &["capacity", "offload", "warm"])]);
    assert_eq!(calls("let hold = format!(\"{north} policy-replay hold\");"), [call(1, &["policy-replay", "hold"])]);
}
