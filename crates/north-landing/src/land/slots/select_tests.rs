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

#[test]
fn the_rust_input_pattern_matches_a_tree_holding_a_rust_build_input_and_nothing_else() {
    let repo = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(repo.path()).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(arguments).status().unwrap().success());
    git(&["init", "-q"]);
    fs_write(repo.path(), "docs/features.md");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "docs"]);
    assert!(!touches(repo.path(), "", "HEAD", "@rust-input"));
    assert!(touches(repo.path(), "", "HEAD", "@rust-input,docs/"));
    fs_write(repo.path(), "harness/model-catalog.toml");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "embedded"]);
    assert!(touches(repo.path(), "HEAD~1", "HEAD", "@rust-input"));
    assert!(!touches(repo.path(), "HEAD~1", "HEAD", "docs/"));
}

fn fs_write(root: &Path, path: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "x\n").unwrap();
}

const SLOTTED: Placement = Placement { workflow: true, runner: true, reserve: false, free_slots: Some(2) };

#[test]
fn a_portable_check_runs_in_a_slot_while_one_is_free_and_goes_to_actions_only_when_every_slot_is_busy() {
    assert!(!SLOTTED.on_actions(true));
    assert!(Placement { free_slots: Some(0), ..SLOTTED }.on_actions(true));
    assert!(!Placement { free_slots: Some(0), ..SLOTTED }.on_actions(false), "a machine-bound check never goes to Actions");
}

#[test]
fn without_a_runner_every_portable_check_goes_to_actions_and_without_a_workflow_none_does() {
    let actions_only = Placement { runner: false, ..SLOTTED };
    assert!(actions_only.on_actions(true));
    assert!(!actions_only.probes_slots());
    assert!(!Placement { workflow: false, free_slots: Some(0), ..SLOTTED }.on_actions(true));
    assert!(!Placement { workflow: false, ..SLOTTED }.probes_slots());
}

#[test]
fn game_reserve_keeps_every_check_on_its_offload_host() {
    let reserve = Placement { reserve: true, free_slots: Some(0), ..SLOTTED };
    assert!(!reserve.on_actions(true));
    assert!(!reserve.probes_slots());
    assert!(!reserve.overflows(OFFLOAD_SLOTS_BUSY, true, true));
}

#[test]
fn only_an_offloaded_portable_check_whose_slot_wait_ran_out_overflows() {
    assert!(SLOTTED.overflows(OFFLOAD_SLOTS_BUSY, true, true));
    assert!(!SLOTTED.overflows(OFFLOAD_SLOTS_BUSY, false, true), "a local check exiting 96 failed");
    assert!(!SLOTTED.overflows(OFFLOAD_SLOTS_BUSY, true, false));
    assert!(!SLOTTED.overflows(1, true, true));
    assert!(!Placement { workflow: false, ..SLOTTED }.overflows(OFFLOAD_SLOTS_BUSY, true, true));
}

#[test]
fn a_runner_whose_host_was_never_probed_keeps_its_checks_off_actions() {
    assert!(!Placement { free_slots: None, ..SLOTTED }.on_actions(true));
}
