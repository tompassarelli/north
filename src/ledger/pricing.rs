use std::collections::HashMap;

use serde::Deserialize;

const PRICES: &str = include_str!("prices.json");

#[derive(Deserialize)]
pub struct PriceTable {
    pub models: HashMap<String, Price>,
}

impl PriceTable {
    pub fn load() -> serde_json::Result<Self> {
        serde_json::from_str(PRICES)
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
    fn every_price_parses() {
        let table = PriceTable::load().unwrap();
        assert!(table.models.contains_key("claude-haiku-5-5") && table.models.contains_key("gpt-6-luna"));
    }
}
