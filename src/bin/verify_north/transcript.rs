use serde_json::Value;

const SHOWN_CHARS: usize = 160;
const WORKING: &str = "working";

pub struct Entry {
    pub kind: String,
    pub text: String,
}

pub struct Transcript {
    pub status: String,
    pub conversation: String,
    pub entries: Vec<Entry>,
}

impl Transcript {
    pub fn parse(session: &Value) -> Self {
        let entries = session["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| Entry {
                kind: entry["kind"].as_str().unwrap_or_default().to_owned(),
                text: entry["text"].as_str().unwrap_or_default().to_owned(),
            })
            .collect();
        let text = |key: &str| session[key].as_str().unwrap_or_default().to_owned();
        Self { status: text("status"), conversation: text("conversation"), entries }
    }

    pub fn settled(&self) -> bool {
        let answered = self.entries.iter().rev().find(|entry| entry.kind == "user" || entry.kind == "assistant").is_some_and(|entry| entry.kind == "assistant");
        self.status != WORKING && answered
    }

    pub fn report(&self) -> Option<String> {
        self.entries.iter().rev().find(|entry| entry.kind == "assistant").map(|entry| entry.text.clone())
    }

    pub fn state(&self) -> String {
        let last = self.entries.iter().rev().find(|entry| entry.kind != "turn");
        let last = last.map_or_else(|| "no entries".to_owned(), |entry| format!("last {} entry {:?}", entry.kind, shorten(&entry.text)));
        format!("status {}, {last}", self.status)
    }
}

pub fn shorten(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SHOWN_CHARS {
        return flat;
    }
    format!("{}…", flat.chars().take(SHOWN_CHARS).collect::<String>())
}
