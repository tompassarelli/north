use north_graph::Telling;
use north_threads::threads::incidents::corrections::classes;
use serde_json::Value;

use crate::facts::{Facts, instant, text};
use crate::places::Places;

const WRITER: &str = "corrections";
const REPLACED: [&str; 4] = ["class-paraphrase", "class-owner", "class-guard", "class-guarded-at"];

pub fn read(places: &Places) -> Result<Telling, String> {
    let text_of = std::fs::read_to_string(&places.corrections).map_err(|why| format!("{}: {why}", places.corrections.display()))?;
    let mut facts = Facts::new(&REPLACED);
    for class in classes() {
        let node = format!("class:{}", class.name);
        facts.tell(WRITER, text(&node), "class-paraphrase", text(&class.paraphrase));
        if let Some(owner) = &class.owner {
            facts.tell(WRITER, text(&node), "class-owner", text(owner));
        }
        if let Some(guard) = &class.guard {
            facts.tell(WRITER, text(&node), "class-guard", text(guard));
        }
        if let Some(at) = class.guarded_at.and_then(instant) {
            facts.tell(WRITER, text(&node), "class-guarded-at", at);
        }
    }
    for row in text_of.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|row| row["kind"] == "correction") {
        tell_correction(&mut facts, &row);
    }
    Ok(facts.finish())
}

fn tell_correction(facts: &mut Facts, row: &Value) {
    let Some(id) = row["id"].as_str() else { return };
    let node = format!("correction:{id}");
    facts.tell(WRITER, text(&node), "class", text(format!("class:{}", row["class"].as_str().unwrap_or("unclassified"))));
    if let Some(at) = row["at"].as_str().and_then(north_base::foundation::clock::Timestamp::parse_loose).and_then(|stamp| instant(stamp.unix_seconds_f64())) {
        facts.tell(WRITER, text(&node), "at", at);
    }
    if let Some(paraphrase) = row["quote"].as_str() {
        facts.tell(WRITER, text(&node), "paraphrase", text(paraphrase));
    }
    let source = row["source"].as_str().unwrap_or_default();
    if let Some(session) = source.split(' ').find_map(|word| word.strip_prefix("session:")) {
        facts.tell(WRITER, text(&node), "in-session", text(session));
    }
    if let Some(later) = row["superseded_by"].as_str() {
        facts.tell(WRITER, text(&node), "superseded-by", text(format!("correction:{later}")));
    }
}
