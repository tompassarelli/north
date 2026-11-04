use super::*;

fn config(checks: &[&str], local_only: &[&str]) -> Config {
    Config {
        checks: checks.iter().map(|entry| (*entry).to_owned()).collect(),
        local_only: local_only.iter().map(|entry| (*entry).to_owned()).collect(),
        ..Config::default()
    }
}

#[test]
fn the_local_reason_is_the_text_before_the_first_separator_of_the_entry_ending_in_the_command() {
    let config = config(&[], &["needs this host :: cargo test", "other :: cargo build"]);
    assert_eq!(config.local_reason("cargo test"), Some("needs this host"));
    assert_eq!(config.local_reason("cargo"), None);
}

#[test]
fn the_publish_ref_is_the_prefix_and_the_first_twelve_characters_of_the_head() {
    let config = Config { publish: Some("farm/".into()), ..Config::default() };
    assert_eq!(config.publish_ref("0123456789abcdef").as_deref(), Some("farm/0123456789ab"));
    assert_eq!(Config::default().publish_ref("0123456789abcdef"), None);
}

#[test]
fn an_entry_without_the_separator_is_invalid() {
    let error = config(&["no separator"], &[]).commands(Path::new("."), "", "HEAD").unwrap_err();
    assert_eq!(error, "invalid land.check: no separator");
}
