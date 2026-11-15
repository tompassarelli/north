//! The owner's Stop hook over an unanswered question: a final message does not end the turn when his last prompt
//! asked a question and the reply's first line answers none of it (north#641).

use crate::guard::{coordinator_role, recommend_acts};
use crate::hook_input::{HookInput, Verdict};
use std::path::{Path, PathBuf};

const UNIT: &str = "unanswered-guard";
const MAX_BLOCKS: u32 = 2;
const QUOTE_CHARS: usize = 120;

const QUESTION_WORDS: [&str; 27] = [
    "did", "do", "does", "is", "are", "was", "were", "have", "has", "had", "can", "could", "will", "would", "should", "shall", "may", "might", "must", "why", "what", "when", "where", "who", "whom", "whose", "how",
];

const QUANTITY_WORDS: [&str; 13] = ["many", "much", "long", "soon", "old", "time", "date", "day", "number", "count", "far", "often", "late"];

const NUMBER_WORDS: [&str; 14] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "half", "twice", "none"];

const OPENERS: [&str; 8] = ["and", "but", "so", "then", "ok", "okay", "now", "well"];

const POLARITY: [&str; 15] = ["yes", "yeah", "yep", "no", "nope", "not", "none", "nothing", "never", "correct", "incorrect", "wrong", "exactly", "indeed", "false"];

const STOPWORDS: [&str; 68] = [
    "the", "and", "but", "for", "with", "from", "that", "this", "these", "those", "there", "here", "what", "which", "who", "whom", "whose", "when", "where", "why", "how", "did", "does", "doing", "you", "your", "yours", "i", "me", "my", "mine", "we", "our", "ours", "it", "its", "they", "them", "their", "he", "she", "his", "her", "was", "were", "are", "is", "be", "been", "being", "have", "has", "had", "can", "could", "will", "would", "should", "shall", "may", "might", "must", "not", "any", "all", "just", "now", "still",
];

pub fn evaluate(input: &HookInput) -> Verdict {
    if !coordinator_role::is_tom_window(&coordinator_role::Live, input) {
        return Verdict::Allow;
    }
    let Some(home) = std::env::var_os("HOME") else { return Verdict::Allow };
    decide(input, &PathBuf::from(home).join(".local/state/north/unanswered"))
}

pub fn decide(input: &HookInput, state: &Path) -> Verdict {
    let Some(mut turn) = input.data["transcript_path"].as_str().and_then(recommend_acts::read_turn) else { return Verdict::Allow };
    if let Some(text) = input.data["last_assistant_message"].as_str() {
        turn.reply = Some(text.to_owned());
    }
    let (Some(prompt), Some(reply)) = (turn.prompt.as_deref(), turn.reply.as_deref()) else { return Verdict::Allow };
    if !asks(prompt) || answers(first_line(reply), prompt) {
        return Verdict::Allow;
    }
    let session = input.data["session_id"].as_str().unwrap_or("unknown").replace(['/', '.'], "_");
    if !count_block(&state.join(session), &turn.id) {
        return Verdict::Allow;
    }
    Verdict::Deny(format!(
        "BLOCKED ({UNIT}): Tom's last prompt asked a question (\"{}\") and this reply's first line answers none of it. Open the reply with the answer itself: yes or no, the number, the name, or the fact the question asked for.",
        quote(prompt)
    ))
}

/// Whether `prompt` asks a question: it holds a question mark or opens with a question word.
pub fn asks(prompt: &str) -> bool {
    let prompt = recommend_acts::unquoted(prompt);
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return false;
    }
    prompt.contains('?') || QUESTION_WORDS.contains(&opening(prompt).as_str())
}

/// Whether `line` answers `prompt`: a yes or no to a yes-or-no question, a number to a question that asks for one, or a name or fact drawn from the prompt's own words.
pub fn answers(line: &str, prompt: &str) -> bool {
    let said: Vec<String> = words(line).iter().map(|word| stem(word)).collect();
    let asked: Vec<String> = words(prompt).iter().map(|word| stem(word)).collect();
    if asked.iter().any(|word| word.len() >= 3 && !STOPWORDS.contains(&word.as_str()) && said.contains(word)) {
        return true;
    }
    let said = words(line);
    if said.iter().any(|word| POLARITY.contains(&word.as_str())) {
        return true;
    }
    asks_quantity(prompt) && (said.iter().any(|word| NUMBER_WORDS.contains(&word.as_str())) || line.chars().any(|character| character.is_ascii_digit()))
}

fn asks_quantity(prompt: &str) -> bool {
    let said = words(prompt);
    let next = |word: &str| said.windows(2).any(|pair| pair[0] == word && QUANTITY_WORDS.contains(&pair[1].as_str()));
    opening(prompt) == "when"
        || next("how")
        || next("what")
        || (opening(prompt) == "which" && said.iter().any(|word| QUANTITY_WORDS.contains(&word.as_str())))
}

fn opening(prompt: &str) -> String {
    let said = words(prompt);
    let mut said = said.iter();
    let first = said.next().cloned().unwrap_or_default();
    if OPENERS.contains(&first.as_str()) { said.next().cloned().unwrap_or(first) } else { first }
}

fn stem(word: &str) -> String {
    for suffix in ["ing", "ed", "es", "s"] {
        if let Some(rest) = word.strip_suffix(suffix).filter(|rest| rest.len() >= 3) {
            return rest.to_owned();
        }
    }
    word.to_owned()
}

fn first_line(reply: &str) -> &str {
    reply.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default()
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|character: char| !(character.is_alphanumeric() || character == '\''))
        .map(|word| word.trim_matches('\'').to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

fn quote(prompt: &str) -> String {
    let line = first_line(prompt);
    let mut quoted: String = line.chars().take(QUOTE_CHARS).collect();
    if line.chars().count() > QUOTE_CHARS {
        quoted.push('…');
    }
    quoted
}

fn count_block(file: &Path, turn: &str) -> bool {
    let recorded = std::fs::read_to_string(file).ok();
    let blocks = recorded.as_deref().and_then(|text| text.split_once('\t')).filter(|(id, _)| *id == turn).and_then(|(_, count)| count.trim().parse::<u32>().ok()).unwrap_or(0);
    if blocks >= MAX_BLOCKS {
        return false;
    }
    let _ = file.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(file, format!("{turn}\t{}\n", blocks + 1));
    true
}
