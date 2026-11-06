use north_graph::Telling;

use crate::facts::{Facts, instant, text};
use crate::places::Places;

const WRITER: &str = "push-log";

pub fn read(places: &Places) -> Result<Telling, String> {
    if !places.pushes.is_file() {
        return Err(format!("{}: no push log", places.pushes.display()));
    }
    let mut facts = Facts::new(&[]);
    for push in north_fleet::fleet::landings::pushes(&places.pushes) {
        let oid = push.oid.to_ascii_lowercase();
        let at = instant(push.at_ms as f64 / 1000.0);
        facts.tell(WRITER, text(&oid), "repo", text(&push.repository));
        facts.tell(WRITER, text(&oid), "branch", text(&push.branch));
        if let Some(at) = at.clone() {
            facts.tell(WRITER, text(&oid), "pushed-at", at);
        }
        if !push.session.is_empty() {
            facts.tell(WRITER, text(&push.session), "pushed", text(&oid));
        }
        if push.landed.is_empty() {
            continue;
        }
        let landed = push.landed.to_ascii_lowercase();
        facts.tell(WRITER, text(&oid), "landed-as", text(&landed));
        facts.tell(WRITER, text(&landed), "repo", text(&push.repository));
        if !push.session.is_empty() {
            facts.tell(WRITER, text(&landed), "landed-by", text(&push.session));
        }
        if let Some(at) = at {
            facts.tell(WRITER, text(&landed), "landed-at", at);
        }
    }
    Ok(facts.finish())
}
