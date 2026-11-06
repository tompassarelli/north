use std::sync::LazyLock;

use super::harness::{self, DEFAULTS};

const PHRASES: &str = "human_signin_phrases";
const NEGATORS: &str = "human_signin_negators";

static LISTS: LazyLock<(Vec<Vec<String>>, Vec<String>)> = LazyLock::new(|| {
    let document = harness::document(DEFAULTS);
    let strings = |name: &str| -> Vec<String> {
        let list = document[name]["value"].as_array().unwrap_or_else(|| panic!("harness/defaults.toml has no list for {name}"));
        list.iter().filter_map(|entry| entry.as_str()).map(str::to_owned).collect()
    };
    (strings(PHRASES).iter().map(|phrase| words(phrase)).filter(|phrase| !phrase.is_empty()).collect(), strings(NEGATORS).iter().flat_map(|negator| words(negator)).collect())
});

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

/// The phrase on the first line of `text` that plans for a person to sign in, enter a password, confirm a passkey or unlock a keyring; a line that also says no, never, without or refuses is describing the ban, so it is skipped.
pub fn plans_human_signin(text: &str) -> Option<String> {
    let (phrases, negators) = &*LISTS;
    text.lines().find_map(|line| {
        let line = words(line);
        if line.iter().any(|word| negators.contains(word)) {
            return None;
        }
        phrases.iter().find(|phrase| line.windows(phrase.len()).any(|window| window == phrase.as_slice())).map(|phrase| phrase.join(" "))
    })
}

/// The first sign-in phrase `text` names, negators ignored: a gate asks Tom something, so mentioning a sign-in there asks for one.
pub fn names(text: &str) -> Option<String> {
    let (phrases, _) = &*LISTS;
    let line = words(text);
    phrases.iter().find(|phrase| line.windows(phrase.len()).any(|window| window == phrase.as_slice())).map(|phrase| phrase.join(" "))
}

const PLANNED: [&str; 3] = ["item", "box", "done when"];

/// The refusal for a launch brief whose Item, Box or Done when header line plans a human sign-in, which tells the lead to file the box that builds the machine path instead.
pub fn brief_refusal(text: &str) -> Option<String> {
    let mut planned = Vec::new();
    let mut inside = false;
    for line in text.lines().take_while(|line| !line.trim().is_empty()) {
        let continued = line.starts_with(char::is_whitespace) || line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ");
        if !continued && let Some((name, _)) = line.split_once(':') && name.chars().all(|character| character.is_ascii_alphabetic() || character == ' ') {
            inside = PLANNED.contains(&name.to_lowercase().as_str());
        }
        if inside {
            planned.push(line);
        }
    }
    let phrase = plans_human_signin(&planned.join("\n"))?;
    Some(format!("the plan needs a human sign-in (\"{phrase}\"), and no agent work depends on one; file a box that builds the machine path (a GitHub App token, a sops-held key or a setup token) and launch that instead"))
}
