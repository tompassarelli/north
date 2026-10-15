use serde_json::Value;

const REPLY_MARK: &str = "• ";
const SHOWN_CHARS: usize = 160;

pub struct Entry {
    pub kind: String,
    pub text: String,
}

pub struct Transcript {
    pub status: String,
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
        Self { status: session["status"].as_str().unwrap_or_default().to_owned(), entries }
    }

    pub fn after(&self, marker: &str) -> Option<&[Entry]> {
        let asked = self.entries.iter().rposition(|entry| entry.kind == "user" && entry.text.contains(marker))?;
        Some(&self.entries[asked + 1..])
    }

    pub fn reply_after(&self, marker: &str) -> Option<String> {
        let replies: Vec<&str> = self.after(marker)?.iter().filter(|entry| entry.kind == "assistant").map(|entry| entry.text.as_str()).collect();
        (!replies.is_empty()).then(|| replies.join("\n"))
    }

    pub fn state(&self) -> String {
        let last = self.entries.iter().rev().find(|entry| entry.kind != "turn");
        let last = last.map_or_else(|| "no entries".to_owned(), |entry| format!("last {} entry {:?}", entry.kind, shorten(&entry.text)));
        format!("status {}, {last}", self.status)
    }
}

pub fn holds_number(text: &str, number: &str) -> bool {
    text.split(|character: char| !character.is_ascii_digit()).any(|token| token == number)
}

pub fn screen_shows_reply(screen: &str, number: &str) -> bool {
    screen.lines().filter_map(|line| line.split_once(REPLY_MARK)).any(|(_, reply)| holds_number(reply, number))
}

pub fn shorten(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SHOWN_CHARS {
        return flat;
    }
    format!("{}…", flat.chars().take(SHOWN_CHARS).collect::<String>())
}
