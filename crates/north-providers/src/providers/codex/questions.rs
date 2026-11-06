use serde_json::{Map, Value, json};

use crate::fleet::gates::{ASKED_REPLY, Ask, Question};

const REQUEST_USER_INPUT: &str = "item/tool/requestUserInput";

pub fn question(frame: &Value) -> Option<Question> {
    if frame["method"] != REQUEST_USER_INPUT {
        return None;
    }
    let id = frame.get("id").filter(|id| id.is_string() || id.is_u64() || id.is_i64())?;
    let params = &frame["params"];
    let conversation = params["threadId"].as_str()?.to_owned();
    let items = params["questions"].as_array()?;
    let asks = items.iter().filter_map(ask).collect();
    let answers: Map<String, Value> = items.iter().filter_map(|item| item["id"].as_str()).map(|key| (key.to_owned(), json!({"answers": [ASKED_REPLY]}))).collect();
    let reply = json!({"id": id, "result": {"answers": answers}}).to_string();
    Some(Question { conversation, asks, reply })
}

fn ask(item: &Value) -> Option<Ask> {
    let question = item["question"].as_str().map(str::trim).filter(|text| !text.is_empty())?;
    let text = match item["header"].as_str().map(str::trim).filter(|header| !header.is_empty() && !question.contains(header)) {
        Some(header) => format!("{header}: {question}"),
        None => question.to_owned(),
    };
    let options = item["options"].as_array().into_iter().flatten().filter_map(|option| option["label"].as_str()).map(str::to_owned).collect();
    Some(Ask { text, options })
}
