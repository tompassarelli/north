use std::fs;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::telemetry::{Prices, run_usage};
use super::{CATALOG, DEFAULTS, roots};

const PRICED: &str = r#"
[models."claude:opus-medium"]
model = "claude-opus-5-5"
[models."claude:opus-medium".value.api]
input_per_million = 4.0
output_per_million = 20.0
[models."claude:haiku"]
model = "claude-haiku-5-5"
[models."claude:haiku".value.api]
input_per_million = 0.1
output_per_million = 0.5
"#;

fn turn(model: &str, request: &str, usage: Value) -> Value {
    json!({"type": "assistant", "timestamp": "2026-10-11T00:05:00Z", "requestId": request, "message": {"model": model, "usage": usage}})
}

fn weight(prices: &Prices, model: &str) -> (f64, Value) {
    let usage = json!({"input_tokens": 1000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 1000, "output_tokens": 100});
    let telemetry = run_usage(&[turn(model, model, usage)], "claude-b", prices);
    (telemetry["usage_samples"][0]["weight"].as_f64().unwrap(), telemetry["unpriced_models"].clone())
}

#[test]
fn a_streamed_request_counts_once_at_token_weight_when_its_model_has_no_list_price() {
    let usage = json!({"input_tokens": 100, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 100, "output_tokens": 100, "speed": "fast"});
    let record = turn("claude-opus-4-6", "r", usage);
    let telemetry = run_usage(&[record.clone(), record], "claude-b", &Prices::parse("", DEFAULTS));
    assert_eq!(telemetry["fast"], 1);
    assert_eq!(telemetry["account"], "claude-b");
    assert_eq!(telemetry["usage_samples"].as_array().unwrap().len(), 1);
    assert_eq!(telemetry["usage_samples"][0]["weight"], 730.0);
    assert_eq!(telemetry["unpriced_models"], json!(["claude-opus-4-6"]));
}

#[test]
fn list_prices_weight_opus_forty_times_haiku_and_an_uncatalogued_model_takes_token_weight() {
    let prices = Prices::parse(PRICED, DEFAULTS);
    let (opus, opus_unpriced) = weight(&prices, "claude-opus-5-5");
    let (haiku, _) = weight(&prices, "claude-haiku-5-5");
    let (sonnet, sonnet_unpriced) = weight(&prices, "claude-sonnet-9");
    assert_eq!((opus, haiku, sonnet), (6400.0, 160.0, 1600.0));
    assert!((opus / haiku - 40.0).abs() < 1e-9);
    assert_eq!(opus_unpriced, json!([]));
    assert_eq!(sonnet_unpriced, json!(["claude-sonnet-9"]));
}

#[test]
fn the_built_in_catalog_prices_every_claude_model_it_names() {
    let prices = Prices::parse(CATALOG, DEFAULTS);
    for model in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] {
        assert_eq!(weight(&prices, model).1, json!([]), "{model}");
    }
}

#[test]
fn a_root_declared_at_claudes_default_home_takes_its_label_from_the_config_beside_it() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude/projects")).unwrap();
    fs::create_dir_all(home.path().join("profile/projects")).unwrap();
    fs::write(home.path().join(".claude.json"), json!({"oauthAccount": {"accountUuid": "account-one"}}).to_string()).unwrap();
    let identity = &format!("{:x}", Sha256::digest(b"account-one"))[..8];
    let policy = format!("[accounts.claude]\nlabels = {{ \"{identity}\" = \"a\" }}\nprofiles = {{ a = {{ here = {{ path = \"~/.claude\" }} }} }}\n");

    let roots = roots(home.path(), Some(home.path().join("profile")), Some(&policy.parse().unwrap()), "here");

    let accounts: Vec<&str> = roots.iter().map(|root| root.account.as_str()).collect();
    assert_eq!(accounts, ["claude", "claude-a"]);
}
