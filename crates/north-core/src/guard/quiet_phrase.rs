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
    pub kinds: Vec<(u64, Vec<String>)>,
    pub extend_words: Vec<String>,
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
            kinds: strings("capacity_lease_kinds").iter().filter_map(|row| kind(row)).collect(),
            extend_words: strings("capacity_phrase_extend_words"),
        }
    }
}

fn kind(row: &str) -> Option<(u64, Vec<String>)> {
    let (minutes, words) = row.split_once(':')?;
    Some((minutes.trim().parse().ok()?, words.split(',').map(|word| word.trim().to_lowercase()).filter(|word| !word.is_empty()).collect()))
}

#[derive(Debug, PartialEq)]
pub enum Phrase {
    On(String),
    Off(String),
    Extend,
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
    if let Some(phrase) = near(&words, &table.on_verbs, &table.on_objects, table.window) {
        return Some(Phrase::On(phrase));
    }
    (words.len() <= 3 && words.iter().any(|word| table.extend_words.contains(word))).then_some(Phrase::Extend)
}

pub fn lease_minutes(prompt: &str, table: &Table, default: u64) -> u64 {
    let words = words(&unquoted(prompt));
    let stated = words.iter().enumerate().find_map(|(at, word)| {
        let digits = word.trim_end_matches(|character: char| character.is_alphabetic());
        let unit = if digits.len() < word.len() { &word[digits.len()..] } else { words.get(at + 1).map_or("", String::as_str) };
        matches!(unit, "m" | "min" | "mins" | "minute" | "minutes").then(|| digits.parse::<u64>().ok()).flatten().filter(|minutes| *minutes > 0)
    });
    let joined = format!(" {} ", words.join(" "));
    stated
        .or_else(|| table.kinds.iter().find(|(_, names)| names.iter().any(|name| joined.contains(&format!(" {name} ")))).map(|(minutes, _)| *minutes))
        .unwrap_or(default)
}

pub fn decide(prompt: &str, table: &Table, limits: &Limits, dir: &Path, now: u64, teardown: impl FnOnce() -> String) -> Option<String> {
    let state = mode::read(dir);
    match classify(prompt, table)? {
        Phrase::On(phrase) => {
            let minutes = lease_minutes(prompt, table, limits.lease_minutes);
            leased(dir, &mode::hold(&state, now, &phrase, minutes, limits), &phrase)
        }
        Phrase::Extend => {
            let extended = mode::extend(&state, now, limits)?;
            let phrase = extended.hold.as_ref().map(|hold| hold.phrase.clone()).unwrap_or_default();
            leased(dir, &extended, &phrase)
        }
        Phrase::Off(phrase) => {
            mode::write(dir, &mode::release(&state, now)).ok()?;
            Some(format!("capacity mode: quiet off from \"{phrase}\": {}; the machine is released to agents within 30 s", teardown()))
        }
    }
}

fn leased(dir: &Path, held: &mode::State, phrase: &str) -> Option<String> {
    mode::write(dir, held).ok()?;
    let hold = held.hold.as_ref()?;
    let until = crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(hold.until).unwrap_or(i64::MAX)).local_twelve_hour();
    Some(format!(
        "capacity mode: playtest lease from \"{phrase}\" until {until} (game CPUs reserved, native and gpu batch held, other agent work confined to the agent CPUs); it returns by itself, \"more\" adds {} min and \"done testing\" ends it now",
        hold.minutes
    ))
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
