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
fn after_lines_stay_out_of_the_pre_land_set_and_make_up_the_after_set() {
    let config = Config { after: vec!["src/ :: cargo test".into()], ..config(&["src/ :: cargo clippy"], &[]) };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(config.commands(repo, "", "HEAD").unwrap(), vec!["cargo clippy".to_owned()]);
    assert_eq!(config.after_set().commands(repo, "", "HEAD").unwrap(), vec!["cargo test".to_owned()]);
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

#[test]
fn a_machine_bound_check_stays_off_actions_without_being_pinned_to_this_host() {
    let config = Config { machine_bound: vec!["reads the checkouts :: owners".into()], ..config(&[], &["needs this host :: cargo test"]) };
    assert_eq!(config.off_actions_reason("owners"), Some("reads the checkouts"));
    assert_eq!(config.local_reason("owners"), None);
    assert_eq!(config.off_actions_reason("cargo test"), Some("needs this host"));
    assert_eq!(config.off_actions_reason("cargo"), None);
}
