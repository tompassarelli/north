use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUpdate {
    pub thread_id: String,
    pub token_usage: ContextUsage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub last: TokenCount,
    pub total: TokenCount,
    pub model_context_window: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCount {
    pub total_tokens: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsage {
    pub rate_limits: QuotaBucket,
    pub rate_limits_by_limit_id: Option<BTreeMap<String, QuotaBucket>>,
    pub rate_limit_reset_credits: Option<ResetCredits>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredits {
    pub available_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBucket {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub used_percent: u64,
    pub window_duration_mins: Option<u64>,
    pub resets_at: Option<u64>,
}

pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
