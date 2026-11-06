use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

pub struct Item {
    pub ticked: bool,
    pub text: String,
    pub head: String,
    pub owner: Option<String>,
    pub landed: Vec<String>,
}

impl Item {
    pub fn id(&self, issue: &str) -> String {
        let digest = Sha256::digest(self.head.as_bytes());
        let short: String = digest.iter().take(6).map(|byte| format!("{byte:02x}")).collect();
        format!("box:{issue}:{short}")
    }
}

fn owner_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\bowner:\s*([A-Za-z0-9][A-Za-z0-9_-]*)").unwrap_or_else(|error| panic!("{error}")))
}

fn landed_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\blanded(?:\s+(?:as|in))?\s+([0-9a-f]{7,40}(?:(?:,\s*(?:and\s+)?|\s+and\s+)[0-9a-f]{7,40})*)\b").unwrap_or_else(|error| panic!("{error}")))
}

fn sha_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"[0-9a-f]{7,40}").unwrap_or_else(|error| panic!("{error}")))
}

pub fn parse(body: &str) -> Vec<Item> {
    body.lines().filter_map(line).collect()
}

fn line(line: &str) -> Option<Item> {
    let line = line.trim_start();
    let rest = line.strip_prefix("- [").or_else(|| line.strip_prefix("* ["))?;
    let (mark, rest) = rest.split_at_checked(1)?;
    let text = rest.strip_prefix("] ")?.trim().to_string();
    let ticked = match mark {
        "x" | "X" => true,
        " " => false,
        _ => return None,
    };
    let owner = owner_pattern().captures(&text).map(|found| found[1].to_string()).filter(|name| !name.eq_ignore_ascii_case("unassigned"));
    let mut landed = Vec::new();
    for found in landed_pattern().captures_iter(&text) {
        for sha in sha_pattern().find_iter(&found[1].to_ascii_lowercase()) {
            if sha.as_str().bytes().any(|byte| byte.is_ascii_digit()) && !landed.iter().any(|known: &String| known == sha.as_str()) {
                landed.push(sha.as_str().to_string());
            }
        }
    }
    let cut = text.to_ascii_lowercase().find(" (owner").unwrap_or(text.len());
    let head: String = text[..cut].chars().take(120).collect();
    Some(Item { ticked, text, head, owner, landed })
}
