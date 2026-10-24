use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub enum StreamFrame {
    Init(SystemInit),
    CommandLifecycle(CommandLifecycle),
    User(UserFrame),
    Assistant(AssistantFrame),
    Result(ResultFrame),
    RateLimit(RateLimitEvent),
    TaskStarted(TaskStarted),
    TaskNotification(TaskNotification),
    ControlResponse(ControlResponse),
    Other(Value),
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SystemInit {
    pub session_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub claude_code_version: Option<String>,
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Queued,
    #[serde(rename = "started")]
    AcceptedIntoTurn,
    Completed,
    Cancelled,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CommandLifecycle {
    pub command_uuid: String,
    pub state: DeliveryState,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UserFrame {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, rename = "isReplay")]
    pub is_replay: bool,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
    #[serde(default)]
    pub message: Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AssistantFrame {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
    #[serde(default)]
    pub user_message_uuids: Vec<String>,
    #[serde(default)]
    pub message: Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ResultFrame {
    pub subtype: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub terminal_reason: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_message_uuids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RateLimitEvent {
    pub rate_limit_info: RateLimitInfo,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RateLimitInfo {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<i64>,
    #[serde(default, rename = "rateLimitType")]
    pub rate_limit_type: Option<String>,
    #[serde(default, rename = "unifiedWindows")]
    pub unified_windows: UnifiedWindows,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct UnifiedWindows {
    #[serde(default)]
    pub five_hour: Option<RateWindow>,
    #[serde(default)]
    pub seven_day: Option<RateWindow>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RateWindow {
    pub utilization: f64,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TaskStarted {
    pub task_id: String,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub task_type: Option<String>,
    #[serde(default)]
    pub is_backgrounded: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TaskNotification {
    pub task_id: String,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ControlResponse {
    pub subtype: String,
    pub request_id: String,
    #[serde(default)]
    pub response: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn decode_frame(line: &str) -> Result<StreamFrame, serde_json::Error> {
    serde_json::from_str(line).map(decode_value)
}

pub fn decode_value(value: Value) -> StreamFrame {
    let kind = value.get("type").and_then(Value::as_str);
    let subtype = value.get("subtype").and_then(Value::as_str);
    let decoded = match (kind, subtype) {
        (Some("system"), Some("init")) => typed(&value).map(StreamFrame::Init),
        (Some("system"), Some("task_started")) => typed(&value).map(StreamFrame::TaskStarted),
        (Some("system"), Some("task_notification")) => {
            typed(&value).map(StreamFrame::TaskNotification)
        }
        (Some("command_lifecycle"), _) => typed(&value).map(StreamFrame::CommandLifecycle),
        (Some("user"), _) => typed(&value).map(StreamFrame::User),
        (Some("assistant"), _) => typed(&value).map(StreamFrame::Assistant),
        (Some("result"), _) => typed(&value).map(StreamFrame::Result),
        (Some("rate_limit_event"), _) => typed(&value).map(StreamFrame::RateLimit),
        (Some("control_response"), _) => value
            .get("response")
            .and_then(typed)
            .map(StreamFrame::ControlResponse),
        _ => None,
    };
    decoded.unwrap_or(StreamFrame::Other(value))
}

fn typed<T: DeserializeOwned>(value: &Value) -> Option<T> {
    T::deserialize(value).ok()
}

pub fn encode_user_message(uuid: &str, text: &str) -> String {
    json!({
        "type": "user",
        "uuid": uuid,
        "session_id": "",
        "parent_tool_use_id": null,
        "message": {"role": "user", "content": text},
    })
    .to_string()
}

pub fn encode_initialize(request_id: &str) -> String {
    control_request(request_id, json!({"subtype": "initialize"}))
}

pub fn encode_interrupt(request_id: &str) -> String {
    control_request(request_id, json!({"subtype": "interrupt"}))
}

fn control_request(request_id: &str, request: Value) -> String {
    json!({"type": "control_request", "request_id": request_id, "request": request}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOLD: &str = include_str!("fixtures/claude-stream/fold.jsonl");
    const INTERRUPT: &str = include_str!("fixtures/claude-stream/interrupt.jsonl");
    const RESUME: &str = include_str!("fixtures/claude-stream/resume.jsonl");

    fn frames(fixture: &str) -> Vec<StreamFrame> {
        fixture
            .lines()
            .map(|line| decode_frame(line).expect("fixture line is JSON"))
            .collect()
    }

    fn lifecycle(frames: &[StreamFrame], uuid: &str) -> Vec<DeliveryState> {
        frames
            .iter()
            .filter_map(|frame| match frame {
                StreamFrame::CommandLifecycle(c) if c.command_uuid == uuid => Some(c.state),
                _ => None,
            })
            .collect()
    }

    fn result(frames: &[StreamFrame]) -> Vec<&ResultFrame> {
        frames
            .iter()
            .filter_map(|frame| match frame {
                StreamFrame::Result(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn fold_fixture_keeps_overlapping_delivery_sequences_apart() {
        let frames = frames(FOLD);
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let order: Vec<(&str, DeliveryState)> = frames
            .iter()
            .filter_map(|f| match f {
                StreamFrame::CommandLifecycle(c) => Some((c.command_uuid.as_str(), c.state)),
                _ => None,
            })
            .collect();
        let second_queued = order.iter().position(|e| *e == (second, DeliveryState::Queued)).unwrap();
        let first_cancelled = order.iter().position(|e| *e == (first, DeliveryState::Cancelled)).unwrap();
        assert!(second_queued < first_cancelled);
        assert_eq!(
            lifecycle(&frames, first),
            [DeliveryState::Queued, DeliveryState::AcceptedIntoTurn, DeliveryState::Cancelled]
        );
        assert_eq!(
            lifecycle(&frames, second),
            [DeliveryState::Queued, DeliveryState::AcceptedIntoTurn, DeliveryState::Completed]
        );
        assert!(frames.iter().any(|f| matches!(
            f,
            StreamFrame::User(u) if u.is_replay && u.uuid.as_deref() == Some(second)
        )));
        let last = result(&frames).pop().unwrap().clone();
        assert_eq!(last.terminal_reason.as_deref(), Some("completed"));
        assert!(last.user_message_uuids.iter().any(|u| u == second));
        let rate = frames
            .iter()
            .find_map(|f| match f {
                StreamFrame::RateLimit(r) => Some(&r.rate_limit_info),
                _ => None,
            })
            .unwrap();
        assert_eq!(rate.unified_windows.five_hour.as_ref().unwrap().utilization, 0.69);
        assert_eq!(rate.unified_windows.seven_day.as_ref().unwrap().utilization, 0.18);
        assert!(rate.unified_windows.seven_day.as_ref().unwrap().resets_at.is_some());
    }

    #[test]
    fn interrupt_fixture_cancels_the_turn_with_aborted_tools() {
        let frames = frames(INTERRUPT);
        let uuid = "55519047-b95b-4d1f-b79b-51e0ecd3b556";
        assert_eq!(
            lifecycle(&frames, uuid),
            [DeliveryState::Queued, DeliveryState::AcceptedIntoTurn, DeliveryState::Cancelled]
        );
        let results = result(&frames);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].terminal_reason.as_deref(), Some("aborted_tools"));
        let started = frames
            .iter()
            .find_map(|f| match f {
                StreamFrame::TaskStarted(t) => Some(t.task_id.clone()),
                _ => None,
            })
            .unwrap();
        assert!(frames.iter().any(|f| matches!(
            f,
            StreamFrame::TaskNotification(n) if n.task_id == started && n.status.as_deref() == Some("stopped")
        )));
        assert!(frames.iter().any(|f| matches!(
            f,
            StreamFrame::ControlResponse(c) if c.request_id == "int1" && c.subtype == "success"
        )));
    }

    #[test]
    fn resume_fixture_decodes_the_resumed_session() {
        let frames = frames(RESUME);
        let init = frames
            .iter()
            .find_map(|f| match f {
                StreamFrame::Init(i) => Some(i),
                _ => None,
            })
            .unwrap();
        assert_eq!(init.session_id, "33333333-3333-4333-8333-333333333333");
        assert_eq!(
            lifecycle(&frames, "55555555-5555-4555-8555-555555555555"),
            [DeliveryState::Queued, DeliveryState::AcceptedIntoTurn, DeliveryState::Completed]
        );
        assert_eq!(result(&frames)[0].result.as_deref(), Some("MARMALADE"));
        assert!(frames.iter().any(|f| matches!(f, StreamFrame::Assistant(_))));
        assert!(!frames.iter().any(|f| matches!(f, StreamFrame::Other(_))));
    }

    #[test]
    fn unknown_frames_and_fields_are_tolerated() {
        let all: Vec<StreamFrame> = [FOLD, INTERRUPT].iter().flat_map(|f| frames(f)).collect();
        let others: Vec<&str> = all
            .iter()
            .filter_map(|f| match f {
                StreamFrame::Other(v) => v.get("subtype").and_then(Value::as_str),
                _ => None,
            })
            .collect();
        assert!(others.contains(&"thinking_tokens"));
        assert!(others.contains(&"hook_started"));
        assert!(others.contains(&"hook_response"));
        assert!(matches!(
            decode_frame(r#"{"type":"brand_new","x":1}"#).unwrap(),
            StreamFrame::Other(_)
        ));
        assert!(matches!(
            decode_frame(r#"{"type":"command_lifecycle","command_uuid":"u","state":"paused","extra":[1]}"#).unwrap(),
            StreamFrame::CommandLifecycle(CommandLifecycle { state: DeliveryState::Unknown, .. })
        ));
        assert!(matches!(
            decode_frame(r#"{"type":"result","terminal_reason":7}"#).unwrap(),
            StreamFrame::Other(_)
        ));
        assert!(decode_frame("not json").is_err());
    }

    #[test]
    fn encoders_write_the_wire_shapes() {
        let user: Value = serde_json::from_str(&encode_user_message("u1", "hi")).unwrap();
        assert_eq!(user["type"], "user");
        assert_eq!(user["uuid"], "u1");
        assert_eq!(user["message"], json!({"role": "user", "content": "hi"}));
        let interrupt: Value = serde_json::from_str(&encode_interrupt("r1")).unwrap();
        assert_eq!(
            interrupt,
            json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "interrupt"}})
        );
        let init: Value = serde_json::from_str(&encode_initialize("r3")).unwrap();
        assert_eq!(init["request"]["subtype"], "initialize");
        assert_eq!(init["request_id"], "r3");
    }
}
