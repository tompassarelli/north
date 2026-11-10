use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const PRICES: &str = include_str!("prices.json");

#[derive(Deserialize)]
pub struct PriceTable {
    models: HashMap<String, Price>,
    aliases: HashMap<String, String>,
}

impl PriceTable {
    pub fn load() -> serde_json::Result<Self> {
        serde_json::from_str(PRICES)
    }

    pub fn price(&self, model: &str) -> Option<Price> {
        let base = model.split_once('[').map_or(model, |(base, _)| base);
        let id = self.aliases.get(base).map_or(base, String::as_str);
        self.models.get(id).copied()
    }

    pub fn run_usd(&self, model: Option<&str>, tier: &str, input: i64, cache_read: i64, output: i64) -> f64 {
        let price = model.and_then(|model| self.price(model)).or_else(|| self.price(fallback_model(tier)));
        price.map_or(0.0, |price| price.of(TokenUse { input: 0, output, cache_read, cache_write: (input - cache_read).max(0) }))
    }
}

#[cfg(test)]
mod tests {
    use super::PriceTable;

    #[test]
    fn every_fable_model_the_fleet_runs_carries_a_price_so_spend_prints_dollars() {
        let prices = PriceTable::load().unwrap();
        let price = prices.price("claude-fable-5-2").expect("claude-fable-5-2 has no price, so a Fable session prints unmeasured");
        assert!(price.of(super::TokenUse { input: 1_000_000, output: 0, cache_read: 0, cache_write: 0 }) > 0.0);
        assert!(prices.run_usd(Some("claude-fable-5-2"), "fable", 1_000_000, 0, 0) > 0.0);
    }
}

fn fallback_model(tier: &str) -> &'static str {
    if tier.contains("haiku") {
        "haiku"
    } else if tier.contains("sonnet") {
        "sonnet"
    } else {
        "opus"
    }
}

#[derive(Clone, Copy, Deserialize)]
pub struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUse {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl Price {
    pub fn of(self, tokens: TokenUse) -> f64 {
        let cost = |count: i64, rate: f64| count as f64 * rate;
        (cost(tokens.input, self.input) + cost(tokens.output, self.output) + cost(tokens.cache_read, self.cache_read) + cost(tokens.cache_write, self.cache_write))
            / 1_000_000.0
    }
}

const COMPACTION_READ_TOKENS: f64 = 450_000.0;
const COMPACTION_SUMMARY_TOKENS: f64 = 20_000.0;
const COMPACTION_REWRITE_TOKENS: f64 = 30_600.0;
const COMPACTED_CONTEXT: u64 = 49_000;

impl Price {
    /// Whether compacting at `context` tokens repays its cost when `turns_left` more turns follow: the context shed is
    /// re-read each turn at the cache read price, against one compaction (a read, a 20k summary, a 30.6k rewrite).
    pub fn compaction_pays(self, context: u64, turns_left: u64) -> bool {
        let cost = (COMPACTION_READ_TOKENS * self.cache_read + COMPACTION_SUMMARY_TOKENS * self.output + COMPACTION_REWRITE_TOKENS * self.cache_write) / 1_000_000.0;
        let saved_per_turn = context.saturating_sub(COMPACTED_CONTEXT) as f64 * self.cache_read / 1_000_000.0;
        turns_left as f64 * saved_per_turn > cost
    }
}

