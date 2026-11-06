use serde_json::Value;

use crate::fleet::model::{Quota, QuotaWindow};

pub(super) fn account_quota(frame: &Value) -> Option<Quota> {
    if frame["type"] != "rate_limit_event" {
        return None;
    }
    let windows = frame["rate_limit_info"]["unifiedWindows"].as_object()?;
    let windows = windows
        .iter()
        .filter_map(|(name, window)| {
            let used = (window["utilization"].as_f64()? * 100.0).round().clamp(0.0, 100.0) as u64;
            Some(QuotaWindow { label: window_label(name), used_percent: used })
        })
        .collect();
    Some(Quota { windows })
}

fn window_label(name: &str) -> String {
    name.replacen("five_hour", "5h", 1).replacen("seven_day", "7d", 1).replace('_', " ")
}
