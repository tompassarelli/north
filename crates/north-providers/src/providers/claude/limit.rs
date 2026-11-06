use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::fleet::model::UsageLimit;

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*you.ve hit your\b[^\n]*\blimit").expect("usage limit pattern"));

pub(super) fn stop_text(text: &str) -> bool {
    STOP.is_match(text)
}

pub(super) fn rejected_window(frame: &Value) -> Option<UsageLimit> {
    let info = &frame["rate_limit_info"];
    if frame["type"] != "rate_limit_event" || info["status"] != "rejected" {
        return None;
    }
    let named = info["rateLimitType"].as_str().and_then(|name| info["unifiedWindows"][name]["resetsAt"].as_u64());
    Some(UsageLimit { resets_at_s: info["resetsAt"].as_u64().or(named), message: None })
}

pub(super) fn rate_limited_result(frame: &Value) -> Option<UsageLimit> {
    let text = frame["result"].as_str();
    let stopped = frame["api_error_status"] == 429 || text.is_some_and(stop_text);
    (frame["type"] == "result" && frame["is_error"] == true && stopped).then(|| UsageLimit { resets_at_s: None, message: text.map(str::to_owned) })
}

pub(super) fn record(limit: UsageLimit, held: &mut Option<UsageLimit>) {
    let before = held.take().unwrap_or_default();
    *held = Some(UsageLimit { resets_at_s: limit.resets_at_s.or(before.resets_at_s), message: limit.message.or(before.message) });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_a_rejected_event_or_an_error_result_carries_a_limit_and_the_event_gives_the_reset() {
        let event = |status: &str| json!({"type": "rate_limit_event", "rate_limit_info": {"status": status, "resetsAt": 1792084800, "rateLimitType": "five_hour"}});
        assert_eq!(rejected_window(&event("rejected")), Some(UsageLimit { resets_at_s: Some(1_792_084_800), message: None }));
        assert_eq!(rejected_window(&event("allowed")), None);
        let named = json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "rateLimitType": "seven_day", "unifiedWindows": {"seven_day": {"resetsAt": 1792670400}}}});
        assert_eq!(rejected_window(&named).unwrap().resets_at_s, Some(1_792_670_400));
        let result = json!({"type": "result", "is_error": true, "api_error_status": 429, "result": "You've hit your limit · resets 3pm"});
        assert_eq!(rate_limited_result(&result).unwrap().message.as_deref(), Some("You've hit your limit · resets 3pm"));
        assert_eq!(rate_limited_result(&json!({"type": "result", "is_error": false, "result": "You've hit your limit"})), None);
        assert_eq!(rate_limited_result(&json!({"type": "result", "is_error": true, "result": "boom"})), None);
    }
}
