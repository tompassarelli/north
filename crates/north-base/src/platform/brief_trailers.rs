//! The `Item:` and `Box:` trailers a brief's lane commits under (north#1367): the launch keeps the brief's item, its box
//! and its Done-when command in the lane for its session, and the commit message, whether the launch proposed it or the
//! worker typed it, takes them here. `Item:` restores the issue on any commit and ticks nothing; `Box:` is added only to
//! the commit of a finish call whose check is the brief's Done-when command, which the call has just run and passed.

const ITEM: &str = "Item:";
const BOX: &str = "Box:";
const CHECK: &str = "Check:";

fn value<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines().rev().find_map(|line| line.strip_prefix(label)).map(str::trim).filter(|value| !value.is_empty())
}

/// The message with the kept `Item:` line added when the message names no item, and the kept `Box:` line added when
/// `check`, the command a finish call ran, is the kept `Check:` command and the message has no `Box:` line. A message
/// that names another item is left alone.
pub fn apply(message: &str, kept: &str, check: Option<&str>) -> String {
    let Some(item) = value(kept, ITEM) else { return message.to_owned() };
    let text = message.trim_end();
    let own = value(text, ITEM);
    if text.is_empty() || own.is_some_and(|own| own.split_whitespace().next() != Some(item)) {
        return message.to_owned();
    }
    let completes = check.zip(value(kept, CHECK)).is_some_and(|(ran, done)| ran.trim() == done) && !text.lines().any(|line| line.starts_with(BOX));
    let mut trailed = text.to_owned();
    if own.is_none() {
        trailed.push_str(&format!("\n\n{ITEM} {item}"));
    }
    if let Some(box_text) = value(kept, BOX).filter(|_| completes) {
        trailed.push_str(&format!("\n{BOX} {box_text}"));
    }
    if trailed == text { message.to_owned() } else { trailed }
}
