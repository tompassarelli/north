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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_for_user_input_becomes_asks_and_an_immediate_reply() {
        let frame = json!({"id": 7, "method": REQUEST_USER_INPUT, "params": {"threadId": "th", "turnId": "t1", "itemId": "q", "questions": [
            {"id": "port", "header": "Port", "question": "Which port should the server use?", "options": [{"label": "8080 (Recommended)", "description": "default"}, {"label": "9090", "description": "alt"}]},
            {"id": "why", "header": "", "question": "Anything else?", "options": null}
        ]}});
        let asked = question(&frame).unwrap();
        assert_eq!(asked.conversation, "th");
        assert_eq!(asked.asks, [
            Ask { text: "Port: Which port should the server use?".into(), options: vec!["8080 (Recommended)".into(), "9090".into()] },
            Ask { text: "Anything else?".into(), options: Vec::new() },
        ]);
        let reply: Value = serde_json::from_str(&asked.reply).unwrap();
        assert_eq!(reply["id"], 7);
        for key in ["port", "why"] {
            assert_eq!(reply["result"]["answers"][key]["answers"][0], ASKED_REPLY, "{key}");
        }
        assert_eq!(question(&json!({"method": "turn/started", "params": {"threadId": "th"}})), None);
        assert_eq!(question(&json!({"method": REQUEST_USER_INPUT, "params": {"threadId": "th", "questions": []}})), None, "a notification without an id cannot be answered");
    }
}
