use std::collections::HashMap;

use serde::Deserialize;

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

#[derive(Clone, Copy, Default)]
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
