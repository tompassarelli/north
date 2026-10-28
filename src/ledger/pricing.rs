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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alias_or_a_context_suffix_resolves_to_its_model_price() {
        let table = PriceTable::load().unwrap();
        let cost = |model: &str| table.price(model).map(|price| price.of(TokenUse { input: 1_000_000, ..TokenUse::default() }));
        assert_eq!(cost("haiku"), Some(0.1));
        assert_eq!(cost("opus[1m]"), cost("claude-opus-5-5"));
        assert_eq!(cost("claude-opus-5-5[1m]"), Some(4.0));
        assert_eq!(cost("gpt-6-luna-mini"), None);
        assert!(table.aliases.values().all(|id| table.models.contains_key(id)), "every alias names a priced model");
    }
}
