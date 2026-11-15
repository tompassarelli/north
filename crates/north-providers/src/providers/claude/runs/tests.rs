use serde_json::json;

use super::telemetry::Prices;


#[test]
fn a_subagent_transcript_adds_its_spend_to_its_parent_run() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("session.jsonl");
    let agents = directory.path().join("session/subagents");
    std::fs::create_dir_all(&agents).unwrap();
    let request = |id: &str, model: &str| {
        json!({"type": "assistant", "requestId": id, "timestamp": "2026-11-06T12:00:00Z", "message": {"model": model, "usage": {"input_tokens": 1_000_000, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}})
    };
    let parent_lines = vec![request("parent-request", "claude-sonnet-5-5")];
    std::fs::write(agents.join("agent-explore.jsonl"), format!("{}\n", request("explore-request", "claude-haiku-5-5"))).unwrap();
    std::fs::write(agents.join("agent-explore.meta.json"), json!({"agentType": "Explore"}).to_string()).unwrap();
    let prices = Prices::parse(&super::CATALOG, &super::DEFAULTS);

    let usage = super::usage_for_run(&parent_lines, &parent, "a", &prices, &|_| false);

    let requests: Vec<&str> = usage["usage_samples"].as_array().unwrap().iter().filter_map(|sample| sample["request"].as_str()).collect();
    assert_eq!(requests, ["parent-request", "explore-request"]);
    assert!(usage["usage_samples"][1]["weight"].as_f64().is_some_and(|weight| weight > 0.0));
}

#[test]
fn the_runs_of_a_worker_on_the_api_account_are_read_from_its_declared_config_dir_north_748() {
    let home = tempfile::tempdir().unwrap();
    let profiles = home.path().join(".local/state/north/profiles");
    let policy: toml_edit::DocumentMut = "[accounts.claude.profiles.a]\nlaptop = { path = \"~/.local/state/north/profiles/claude-a\", token_file = \"~/tokens/claude-a\" }\n[accounts.claude.api_keys.api]\nlaptop = { path = \"~/.local/state/north/profiles/claude-api\" }\nother = { path = \"~/elsewhere\" }\n".parse().unwrap();
    for profile in ["claude-a", "claude-api", "claude-z"] {
        std::fs::create_dir_all(profiles.join(profile).join("projects/-home-tom-code-north")).unwrap();
        std::fs::write(profiles.join(profile).join("projects/-home-tom-code-north/run.jsonl"), "{}\n").unwrap();
    }
    let roots = super::roots(home.path(), None, Some(&policy), "laptop");
    let read: Vec<(std::path::PathBuf, String)> = roots.iter().map(|root| (root.projects.clone(), root.account.clone())).collect();
    assert!(read.contains(&(profiles.join("claude-api/projects"), "claude-api".to_owned())), "{read:?}");
    assert!(read.contains(&(profiles.join("claude-a/projects"), "claude-a".to_owned())), "{read:?}");
    assert!(!read.iter().any(|(projects, _)| projects.starts_with(home.path().join("elsewhere"))), "{read:?}");
    let options = super::Options { session: None, since: String::new(), settled: Default::default(), recorded: Default::default() };
    assert_eq!(super::unread(&profiles, &roots, &options), vec![profiles.join("claude-z")]);
}

#[test]
fn a_run_in_a_scratch_directory_that_landed_a_commit_is_stored_as_work_not_a_probe_north_1103() {
    let project = std::path::Path::new("-tmp-north-verify-north-1794324732-lane/0f1e.jsonl");
    let landed = serde_json::Map::from_iter([("landed".to_owned(), json!(1)), ("landed_sha".to_owned(), json!("72f618c352e39e967c72e473ddb57b5db9f9e803"))]);
    assert!(!super::scratch_probe(&landed, project), "on 2026-11-10 verify-north's worker landed 72f618c352 from a scratch lane, was stored as a probe, and the settle left it not done");
    assert!(super::scratch_probe(&serde_json::Map::from_iter([("landed".to_owned(), json!(0))]), project));
}
