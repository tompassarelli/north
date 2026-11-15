use north_base::foundation::limit_clock::limit_reply;
use serde_json::Value;

use crate::fleet::model::UsageLimit;

pub(super) fn stop_text(text: &str) -> bool {
    limit_reply(text).is_some()
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
