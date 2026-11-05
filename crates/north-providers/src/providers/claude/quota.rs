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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_rate_limit_event_reports_each_unified_window_used() {
        let frame = json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "unifiedWindows": {
            "five_hour": {"utilization": 0.63, "resetsAt": 1792084800},
            "seven_day_opus": {"utilization": 0.04, "resetsAt": 1792670400}
        }}});
        let quota = account_quota(&frame).unwrap();
        let windows: Vec<(&str, u64)> = quota.windows.iter().map(|window| (window.label.as_str(), window.left_percent())).collect();
        assert_eq!(windows, [("5h", 37), ("7d opus", 96)]);
        assert_eq!(account_quota(&json!({"type": "assistant"})), None);
    }
}

