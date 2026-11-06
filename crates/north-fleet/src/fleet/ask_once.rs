use std::sync::LazyLock;

use toml_edit::DocumentMut;

const CLASS_PREFIX: &str = "ask_once_";

static CLASSES: LazyLock<Vec<(String, Vec<Vec<String>>)>> = LazyLock::new(|| {
    let document: DocumentMut = north_base::platform::harness::table(north_base::platform::harness::DEFAULTS).parse().expect("harness/defaults.toml parses");
    document
        .iter()
        .filter_map(|(name, item)| {
            let class = name.strip_prefix(CLASS_PREFIX)?;
            let phrases = item["value"].as_array().unwrap_or_else(|| panic!("harness/defaults.toml has no list for {name}"));
            Some((class.to_owned(), phrases.iter().filter_map(|phrase| phrase.as_str()).map(words).filter(|phrase| !phrase.is_empty()).collect()))
        })
        .collect()
});

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

/// The ask-once class (cost, accounts, credentials, public, deletion or direction) a gate's question or options name, which only Tom decides, so such a gate carries no default.
pub fn class<'a>(question: &'a str, options: impl IntoIterator<Item = &'a String>) -> Option<&'static str> {
    let text: Vec<String> = std::iter::once(question).chain(options.into_iter().map(String::as_str)).flat_map(words).collect();
    CLASSES.iter().find(|(_, phrases)| phrases.iter().any(|phrase| text.windows(phrase.len()).any(|window| window == phrase.as_slice()))).map(|(class, _)| class.as_str())
}
