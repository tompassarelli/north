use serde_json::Value;

pub trait UsageSink: Send + Sync {
    fn observe(&self, provider: &str, account: &str, frame: &Value);
}

pub struct NoUsage;

impl UsageSink for NoUsage {
    fn observe(&self, _provider: &str, _account: &str, _frame: &Value) {}
}
