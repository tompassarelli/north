use crate::guard::permission_offer::proxy_report;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::sync::OnceLock;

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
    let now = crate::foundation::clock::Timestamp::now().unix_seconds_f64();
    decide(input, &session, &|paragraph| crate::guard::host::recorded_decisions(paragraph, now))
}

pub fn decide(input: &HookInput, session: &str, records: &dyn Fn(&str) -> Vec<String>) -> Verdict {
    let Some(paragraph) = proxy_report(input, session).as_deref().and_then(recommendation) else {
        return Verdict::Allow;
    };
    let named = records(&paragraph);
    if named.is_empty() {
        return Verdict::Allow;
    }
    Verdict::Deny(reason(&named))
}

pub fn reason(named: &[String]) -> String {
    format!("These recorded decisions bear on your recommendation: {}; read them and confirm or revise.", named.join(", "))
}

pub fn recommendation(text: &str) -> Option<String> {
    let text = text.replace('\u{2019}', "'").replace("\r\n", "\n");
    paragraphs(&text).into_iter().rev().find(|paragraph| marker().is_match(paragraph)).map(str::to_owned)
}

fn marker() -> &'static Regex {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    MARKER.get_or_init(|| Regex::new(r"(?i)(?:^|[^\w'])(?:i recommend|recommended:|the fix (?:is|would be))(?:[^\w']|$)").unwrap())
}

fn paragraphs(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut end = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            if let Some(from) = start.take() {
                out.push(text[from..end].trim_end());
            }
        } else {
            start.get_or_insert(at);
            end = at + line.len();
        }
        at += line.len();
    }
    if let Some(from) = start {
        out.push(text[from..end].trim_end());
    }
    out
}
