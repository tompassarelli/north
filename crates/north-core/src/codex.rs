use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatUpdate {
    pub conversation: String,
    pub turn: String,
    pub key: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub append: bool,
}

/// Convert app-server item payloads at the foreign boundary. Ordering and
/// replacement of these observations belongs to the checked application.
pub fn chat_updates(message: &Value) -> Vec<ChatUpdate> {
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    let Some(conversation) = params["threadId"].as_str() else { return Vec::new(); };
    let turn = params["turnId"].as_str().or_else(|| params["turn"]["id"].as_str()).unwrap_or_default();
    let update = |key: &str, kind: &str, text: String, status: &str, append: bool| ChatUpdate {
        conversation: conversation.into(), turn: turn.into(), key: key.into(),
        kind: kind.into(), text, status: status.into(), append,
    };
    let item_update = |item: &Value| decode_chat_item(conversation, turn, item,
        if method == "item/started" { "inProgress" } else { "completed" });
    match method {
        "item/started" | "item/completed" => item_update(&params["item"]).into_iter().collect(),
        "item/agentMessage/delta" | "item/commandExecution/outputDelta" => {
            let (Some(key), Some(delta)) = (params["itemId"].as_str(), params["delta"].as_str()) else { return Vec::new(); };
            let kind = if method == "item/agentMessage/delta" { "agentMessage" } else { "commandExecution" };
            vec![update(key, kind, delta.into(), "inProgress", true)]
        }
        "turn/diff/updated" => params["diff"].as_str().map(|diff| update("turn-diff", "diff", diff.into(), "completed", false)).into_iter().collect(),
        "turn/completed" => params["turn"]["items"].as_array().into_iter().flatten().filter_map(item_update).collect(),
        _ => Vec::new(),
    }
}

fn decode_chat_item(conversation: &str, turn: &str, item: &Value, default_status: &str) -> Option<ChatUpdate> {
    let kind = item["type"].as_str()?;
    let key = if kind == "userMessage" { item["clientId"].as_str().or_else(|| item["id"].as_str())? }
        else { item["id"].as_str()? };
    let status = item["status"].as_str().unwrap_or(default_status);
    let text = match kind {
        "userMessage" => decode_user_message(item).ok()??,
        "agentMessage" => item["text"].as_str()?.to_owned(),
        "commandExecution" => {
            let command = item["command"].as_str()?;
            let output = item["aggregatedOutput"].as_str().unwrap_or_default();
            format!("{command}\n{output}")
        }
        "fileChange" => item["changes"].as_array()?.iter().map(|change| {
            let path = change["path"].as_str().unwrap_or_default();
            let diff = change["diff"].as_str().unwrap_or_default();
            format!("{path}\n{diff}")
        }).collect::<Vec<_>>().join("\n"),
        _ => return None,
    };
    Some(ChatUpdate { conversation: conversation.into(), turn: turn.into(), key: key.into(), kind: kind.into(), text, status: status.into(), append: false })
}

fn decode_user_message(item: &Value) -> Result<Option<String>, String> {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "userMessage omitted content".to_string())?;
    let mut parts = Vec::new();
    let mut image_number = 1;
    for input in content {
        match input.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = input.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    parts.push(text.to_owned());
                }
            }
            Some("image" | "localImage") => {
                parts.push(format!("[Image #{image_number}]"));
                image_number += 1;
            }
            Some("audio" | "localAudio") => parts.push("[Audio]".into()),
            _ => {}
        }
    }
    let message = parts.join(" ");
    Ok((!message.is_empty()).then_some(message))
}
