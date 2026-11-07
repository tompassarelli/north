//! What a benchmark arm spent on a pool key, written to the ledger as runs of the org whose key served it, so the pool counts it against that org's grant.
use north_base::error::NorthError;
use north_fleet::fleet::turns::{RunRecord, RunSink, Tokens, Turn, TurnExit};
use north_providers::ledger::Ledger;
use serde_json::Value;

/// One arm's key usage: when it ran and the tokens the API billed per model, read from the cumulative `modelUsage` of its last result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spent {
    pub name: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub models: Vec<(String, Tokens)>,
}

impl Spent {
    pub fn of(name: &str, results: &[Value], started_ms: u64, ended_ms: u64) -> Self {
        let usage = results.iter().rev().find_map(|result| result["modelUsage"].as_object().filter(|models| !models.is_empty()));
        let models = usage
            .into_iter()
            .flatten()
            .map(|(model, tokens)| {
                let count = |field: &str| tokens[field].as_u64().unwrap_or(0);
                (model.clone(), Tokens { input: count("inputTokens"), output: count("outputTokens"), cache_read: count("cacheReadInputTokens"), cache_write: count("cacheCreationInputTokens") })
            })
            .collect();
        Self { name: name.to_owned(), started_ms, ended_ms, models }
    }
}

/// Writes one run per model of `spent` to `ledger` on `api:ORG`, the organization named by the key the arm ran on.
pub fn charge(ledger: &Ledger, org: &str, spent: &Spent) -> Result<(), NorthError> {
    for (model, tokens) in &spent.models {
        let turn = Turn {
            conversation: spent.name.clone(),
            model: Some(model.clone()),
            effort: None,
            prompt: None,
            started_ms: spent.started_ms,
            ended_ms: spent.ended_ms,
            wall_ms: spent.ended_ms.saturating_sub(spent.started_ms),
            turns: 0,
            tokens: *tokens,
            exit: TurnExit::Completed,
        };
        ledger.record_run(&RunRecord { session: format!("{}:{model}", spent.name), task_ref: None, provider: "claude".into(), account: Some(format!("api:{org}")), system_prompt_sha: None, deepen: false, turn })?;
    }
    Ok(())
}
