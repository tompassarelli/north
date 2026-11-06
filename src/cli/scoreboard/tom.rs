use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::guard::recommend_acts::{self, Turn, turn_in};
use crate::ledger::outcome::attention::{Typed, references};
use crate::ledger::outcome::read::Places;
use crate::ops::flow::typed::typed_messages;

const HOUR_S: i64 = 3_600;

/// One turn of a proxy conversation, found by the convo index and parsed from its transcript lines by the recommend-acts hook's own reader when it matters here.
struct ProxyTurn {
    at: i64,
    tom: bool,
    first_line: i64,
    end_line: Option<i64>,
    recommends: bool,
    turn: Option<Turn>,
}

impl ProxyTurn {
    fn wanted(&self) -> bool {
        self.tom || self.recommends
    }
}

/// One typed message that moved issues: which issues, and the seconds until the first of them landed, if one has.
pub struct Ask {
    pub items: BTreeSet<String>,
    pub seconds: Option<f64>,
}

pub struct Tom {
    typed: Vec<Typed>,
    corrections: Vec<i64>,
    turns: Vec<ProxyTurn>,
}

fn grounding_corrections(path: &Path, typed: &BTreeSet<i64>) -> Vec<i64> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["kind"] == "correction")
        .filter(|row| {
            let mut words = row["source"].as_str().unwrap_or_default().split(' ');
            words.next() == Some("tom") && words.next().and_then(|word| word.strip_prefix("convo:")).and_then(|id| id.parse::<i64>().ok()).is_some_and(|id| typed.contains(&id))
        })
        .filter_map(|row| Timestamp::parse_rfc3339(row["at"].as_str()?).map(Timestamp::unix_seconds))
        .collect()
}

fn recommends(reply: &[String]) -> bool {
    !reply.is_empty() && recommend_acts::recommendation(&reply.join("\n\n"), &Turn::default()).is_some()
}

fn conversation_turns(db: &Connection, path: &str, typed: &BTreeMap<i64, i64>) -> rusqlite::Result<Vec<ProxyTurn>> {
    let first = typed.keys().next().copied().unwrap_or_default();
    let mut statement = db.prepare(
        "SELECT m.id, m.line, m.ts, m.role, m.tool, b.text FROM files f JOIN msg m INDEXED BY msg_file ON m.file_id = f.id JOIN body b ON b.id = m.body \
         WHERE f.path = ?1 AND m.dup = 0 AND m.line >= (SELECT line FROM msg WHERE id = ?2) ORDER BY m.line",
    )?;
    let rows = statement.query_map((path, first), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?))
    })?;
    let (mut turns, mut reply): (Vec<ProxyTurn>, Vec<String>) = (Vec::new(), Vec::new());
    for row in rows {
        let (id, line, at, role, tool, text) = row?;
        match (role.as_deref(), tool) {
            (Some("user"), None) => {
                if let Some(open) = turns.last_mut() {
                    open.end_line = Some(line);
                    open.recommends = recommends(&reply);
                }
                reply.clear();
                let at = typed.get(&id).copied().or_else(|| at.as_deref().and_then(Timestamp::parse_rfc3339).map(Timestamp::unix_seconds));
                if let Some(at) = at {
                    turns.push(ProxyTurn { at, tom: typed.contains_key(&id), first_line: line, end_line: None, recommends: false, turn: None });
                }
            }
            (_, Some(_)) => reply.clear(),
            (Some("assistant"), None) => reply.push(text),
            _ => {}
        }
    }
    if let Some(open) = turns.last_mut() {
        open.recommends = recommends(&reply);
    }
    Ok(turns)
}

fn parse_turns(path: &str, turns: &mut [ProxyTurn]) {
    let Ok(file) = fs::File::open(path) else { return };
    let mut wanted = turns.iter_mut().filter(|turn| turn.wanted()).peekable();
    let mut slice: Vec<u8> = Vec::new();
    for (index, line) in BufReader::new(file).split(b'\n').enumerate() {
        let number = index as i64 + 1;
        let Some(turn) = wanted.peek_mut() else { return };
        if number < turn.first_line {
            continue;
        }
        let Ok(line) = line else { return };
        slice.extend_from_slice(&line);
        slice.push(b'\n');
        if turn.end_line.is_some_and(|end| number + 1 >= end) {
            turn.turn = turn_in(&slice);
            slice.clear();
            wanted.next();
        }
    }
    if let Some(turn) = wanted.next() {
        turn.turn = turn_in(&slice);
    }
}

impl Tom {
    pub fn read(places: &Places, home: &Path, since: i64, now: i64) -> Result<Self, String> {
        let typed = typed_messages(&places.convo, home, since, now);
        let ids: BTreeSet<i64> = typed.iter().map(|typed| typed.id).collect();
        let corrections = grounding_corrections(&places.grounding, &ids);
        let mut by_conversation: BTreeMap<&str, BTreeMap<i64, i64>> = BTreeMap::new();
        for message in &typed {
            by_conversation.entry(message.session.as_str()).or_default().insert(message.id, message.at);
        }
        let mut turns = Vec::new();
        if !by_conversation.is_empty() {
            let failed = |why: rusqlite::Error| format!("{}: {why}", places.convo.display());
            let db = Connection::open_with_flags(&places.convo, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
            for (path, messages) in &by_conversation {
                let mut found = conversation_turns(&db, path, messages).map_err(failed)?;
                parse_turns(path, &mut found);
                turns.extend(found);
            }
        }
        Ok(Self { typed, corrections, turns })
    }

    /// The turns that answered a message Tom typed in the window, with their final replies.
    pub fn answered_turns(&self, window: Window) -> Vec<&Turn> {
        self.turns.iter().filter(|proxy| proxy.tom && window.holds(proxy.at)).filter_map(|proxy| proxy.turn.as_ref()).filter(|turn| turn.reply.is_some()).collect()
    }

    pub fn corrections(&self, weeks: [Window; 2]) -> Measure {
        let figure = |window: Window| {
            let (count, hours) = self.correcting(window);
            Figure::Rate { count, hours: hours as f64 }
        };
        Measure {
            name: "Tom's corrections per active hour".into(),
            better: Better::Lower,
            source: "grounding ledger correction rows on Tom's typed messages, over clock hours holding one of those messages (convo store, proxy conversations)",
            weeks: weeks.map(figure),
        }
    }

    pub fn asked(&self, window: Window, firsts: &HashMap<String, Vec<i64>>) -> Vec<Ask> {
        let mut asked = Vec::new();
        for (at, turn) in self.turns.iter().filter(|proxy| proxy.tom && window.holds(proxy.at)).filter_map(|proxy| Some((proxy.at, proxy.turn.as_ref()?))) {
            let launched = turn.commands.iter().filter(|command| command.contains("north fleet launch") || command.contains("north fleet send")).flat_map(|command| references(command));
            let items: BTreeSet<String> = turn.moved.iter().flat_map(|issue| references(&issue.label())).chain(launched).collect();
            if items.is_empty() {
                continue;
            }
            let landed = items.iter().filter_map(|item| firsts.get(item)?.iter().copied().find(|landed| *landed >= at)).min();
            asked.push(Ask { items, seconds: landed.map(|landed| (landed - at) as f64) });
        }
        asked
    }

    pub fn correcting(&self, window: Window) -> (usize, usize) {
        let hours: BTreeSet<i64> = self.typed.iter().filter(|typed| window.holds(typed.at)).map(|typed| typed.at.div_euclid(HOUR_S)).collect();
        (self.corrections.iter().filter(|at| window.holds(**at)).count(), hours.len())
    }

    pub fn asks(&self, weeks: [Window; 2], firsts: &HashMap<String, Vec<i64>>) -> Measure {
        let figure = |window: Window| {
            let asked = self.asked(window, firsts);
            let seconds = asked.iter().filter_map(|ask| ask.seconds).collect();
            let open = asked.iter().filter(|ask| ask.seconds.is_none()).count();
            Figure::Spread { seconds, open, open_label: "not landed yet" }
        };
        Measure {
            name: "Tom's ask to its first landed commit".into(),
            better: Better::Lower,
            source: "each typed message whose turn created, edited or staffed an issue (its transcript, read as north hook recommend-acts reads it) to the first worker-ledger run landing that issue on main (threads runs.landed_at)",
            weeks: weeks.map(figure),
        }
    }

    pub fn recommendations(&self, weeks: [Window; 2]) -> Measure {
        let figure = |window: Window| {
            let judged: Vec<bool> = self
                .turns
                .iter()
                .filter(|proxy| proxy.recommends && window.holds(proxy.at))
                .filter_map(|proxy| proxy.turn.as_ref())
                .filter_map(|turn| recommend_acts::recommendation(turn.reply.as_deref()?, turn))
                .map(|judged| judged.acted)
                .collect();
            Figure::Share { hits: judged.iter().filter(|acted| **acted).count(), of: judged.len(), pending: 0, pending_label: "" }
        };
        Measure {
            name: "proxy recommendations filed as an issue the same turn".into(),
            better: Better::Higher,
            source: "proxy turns whose final reply recommends or asks (north hook recommend-acts' rule, ask-once decisions excluded) that created or edited an issue the reply cites (transcripts)",
            weeks: weeks.map(figure),
        }
    }
}
