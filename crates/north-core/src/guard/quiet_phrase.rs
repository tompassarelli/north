use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::capacity::mode::{self, Limits, strings};
use crate::hook::{HookInput, Verdict};

#[derive(Clone, Debug)]
pub struct Table {
    pub on_verbs: Vec<String>,
    pub on_objects: Vec<String>,
    pub off_verbs: Vec<String>,
    pub off_objects: Vec<String>,
    pub off_phrases: Vec<String>,
    pub window: usize,
    pub max_words: usize,
}

impl Table {
    pub fn bundled() -> Self {
        Self {
            on_verbs: strings("capacity_phrase_on_verbs"),
            on_objects: strings("capacity_phrase_on_objects"),
            off_verbs: strings("capacity_phrase_off_verbs"),
            off_objects: strings("capacity_phrase_off_objects"),
            off_phrases: strings("capacity_phrase_off_phrases"),
            window: mode::number("capacity_phrase_window_words") as usize,
            max_words: mode::number("capacity_phrase_max_words") as usize,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Phrase {
    On(String),
    Off(String),
}

fn unquoted(prompt: &str) -> String {
    let mut kept = String::new();
    let mut quote = None;
    for character in prompt.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if matches!(character, '"' | '`' | '\u{201c}') => quote = Some(if character == '\u{201c}' { '\u{201d}' } else { character }),
            None => kept.push(character),
        }
    }
    kept
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|character: char| !(character.is_alphanumeric() || character == '\'')).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

fn near(words: &[String], verbs: &[String], objects: &[String], window: usize) -> Option<String> {
    words.iter().enumerate().filter(|(_, word)| verbs.contains(word)).find_map(|(at, verb)| {
        words.iter().skip(at + 1).take(window).find(|word| objects.contains(word)).map(|object| format!("{verb} {object}"))
    })
}

pub fn classify(prompt: &str, table: &Table) -> Option<Phrase> {
    if prompt.contains('<') {
        return None;
    }
    let text = unquoted(prompt);
    let words = words(&text);
    if words.is_empty() || words.len() > table.max_words {
        return None;
    }
    let joined = words.join(" ");
    if let Some(phrase) = table.off_phrases.iter().find(|phrase| format!(" {joined} ").contains(&format!(" {} ", phrase.to_lowercase()))) {
        return Some(Phrase::Off(phrase.clone()));
    }
    if let Some(phrase) = near(&words, &table.off_verbs, &table.off_objects, table.window) {
        return Some(Phrase::Off(phrase));
    }
    near(&words, &table.on_verbs, &table.on_objects, table.window).map(Phrase::On)
}

pub fn decide(prompt: &str, table: &Table, limits: &Limits, dir: &Path, now: u64, teardown: impl FnOnce() -> String) -> Option<String> {
    let state = mode::read(dir);
    match classify(prompt, table)? {
        Phrase::On(phrase) => {
            let held = mode::hold(&state, now, &phrase, limits);
            mode::write(dir, &held).ok()?;
            let until = held.hold.as_ref().map_or(now, |hold| hold.until);
            Some(format!(
                "capacity mode: quiet on from \"{phrase}\" (game CPUs reserved, batch held) until {}; it ends when Tom says he is done or the game has been closed {} min",
                crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(until).unwrap_or(i64::MAX)).local_twelve_hour(),
                limits.release_seconds / 60
            ))
        }
        Phrase::Off(phrase) => {
            mode::write(dir, &mode::release(&state, now)).ok()?;
            Some(format!("capacity mode: quiet off from \"{phrase}\": {}; the machine is released to agents within 30 s", teardown()))
        }
    }
}

fn fleet_session(session: &str) -> bool {
    let dir = std::env::var_os("NORTH_FLEET_DIR")
        .map_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/state/north/fleet"), PathBuf::from);
    let needle = format!("\"conversation\": \"{session}\"");
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|entry| std::fs::read_to_string(entry.path().join("session.json")).is_ok_and(|text| text.contains(&needle)))
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if input.data.get("agent_id").is_some() || input.data.get("agent_type").is_some() {
        return Verdict::Allow;
    }
    let Some(prompt) = input.data["prompt"].as_str() else { return Verdict::Allow };
    if input.data["session_id"].as_str().is_some_and(fleet_session) {
        return Verdict::Allow;
    }
    let teardown = || {
        let proc = Path::new("/proc");
        let grace = Duration::from_secs(mode::number("capacity_teardown_grace_sec") as u64);
        crate::capacity::teardown::run(proc, &crate::capacity::teardown::Names::bundled(), &mut crate::capacity::teardown::Live { proc }, grace)
    };
    decide(prompt, &Table::bundled(), &Limits::bundled(), &mode::state_dir(), mode::now(), teardown).map_or(Verdict::Allow, Verdict::Advise)
}
