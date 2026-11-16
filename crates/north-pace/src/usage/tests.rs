use std::fs;

#[test]
fn an_api_key_account_profile_gets_the_shared_harness_so_its_workers_run_under_the_same_hooks() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(home.path().join(".claude/settings.json"), "{}").unwrap();
    let keyed = home.path().join("profiles/claude-api");
    let cfg = serde_json::json!({"accounts": {"claude": {"api_keys": {"api": {"laptop": {"path": keyed.to_str().unwrap()}}}}}});
    north_base::profiles::sync(&cfg, "laptop", home.path()).unwrap();
    assert_eq!(fs::read_link(keyed.join("settings.json")).unwrap(), home.path().join(".claude/settings.json"));
}
