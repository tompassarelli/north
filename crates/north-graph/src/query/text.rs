use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::budget::Budget;
use super::relation::Row;
use crate::error::{QueryError, Result};
use crate::key::{Key, decode_key};
use crate::store::Terms;
use crate::term::{Atom, Term};

const TRIGRAM: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRelation {
    Match,
    Phrase,
    Substring,
    Stem,
    Search,
}

impl TextRelation {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "text-match" => Some(Self::Match),
            "text-phrase" => Some(Self::Phrase),
            "text-substring" => Some(Self::Substring),
            "text-stem" => Some(Self::Stem),
            "text-search" => Some(Self::Search),
            _ => None,
        }
    }

    pub fn arity(self) -> usize {
        if self == Self::Search { 4 } else { 3 }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Match => "text-match",
            Self::Phrase => "text-phrase",
            Self::Substring => "text-substring",
            Self::Stem => "text-stem",
            Self::Search => "text-search",
        }
    }
}

/// Tokenizer v0: maximal runs of letters and digits, lowercased without locale, each token once in first-seen order.
pub fn tokens(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for token in words(text) {
        if !found.contains(&token) {
            found.push(token);
        }
    }
    found
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|character: char| !(character.is_alphabetic() || character.is_numeric())).filter(|word| !word.is_empty()).map(str::to_lowercase)
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// A text clause's bound arguments: the entity and attribute ids when bound, and the needle's text.
pub struct Probe<'a> {
    pub entity: Option<i64>,
    pub attribute: Option<i64>,
    pub needle: &'a str,
}

struct Hit {
    entity: i64,
    attribute: i64,
    value: i64,
    rank: f64,
}

/// The live (entity, attribute) pairs whose String value satisfies `relation`, each with its best score.
pub fn fetch(connection: &Connection, terms: &mut Terms<'_>, relation: TextRelation, probe: &Probe<'_>, budget: &Budget) -> Result<Vec<(i64, i64, f64)>> {
    if tokens(probe.needle).is_empty() {
        return Err(QueryError::Invalid(format!("{} needle {:?} has no letter or digit", relation.name(), probe.needle)).into());
    }
    let tiers: &[(TextRelation, f64)] = match relation {
        TextRelation::Search => &[(TextRelation::Match, 2.0), (TextRelation::Stem, 1.0), (TextRelation::Substring, 0.0)],
        other => &[(other, 0.0)],
    };
    let mut best: HashMap<(i64, i64), f64> = HashMap::new();
    let mut order: Vec<(i64, i64)> = Vec::new();
    for (tier, floor) in tiers {
        for hit in tier_hits(connection, terms, *tier, probe, budget)? {
            let score = floor + hit.rank / (1.0 + hit.rank);
            let pair = (hit.entity, hit.attribute);
            match best.get_mut(&pair) {
                Some(known) => *known = known.max(score),
                None => {
                    best.insert(pair, score);
                    order.push(pair);
                }
            }
        }
    }
    Ok(order.into_iter().map(|pair| (pair.0, pair.1, best[&pair])).collect())
}

fn tier_hits(connection: &Connection, terms: &mut Terms<'_>, relation: TextRelation, probe: &Probe<'_>, budget: &Budget) -> Result<Vec<Hit>> {
    let folded = probe.needle.to_lowercase();
    let (table, expression) = match relation {
        TextRelation::Match | TextRelation::Stem | TextRelation::Search => {
            let table = if relation == TextRelation::Stem { "text_porter" } else { "text_words" };
            (table, tokens(probe.needle).iter().map(|token| quoted(token)).collect::<Vec<_>>().join(" AND "))
        }
        TextRelation::Phrase => ("text_words", quoted(&words(probe.needle).collect::<Vec<_>>().join(" "))),
        TextRelation::Substring if folded.chars().count() < TRIGRAM => return scan(connection, probe, &folded, budget),
        TextRelation::Substring => ("text_trigram", trigrams(&folded)),
    };
    let mut sql = format!("SELECT p.t1, p.t2, p.t3, -bm25({table}) FROM {table} JOIN live_props p ON p.prop = {table}.rowid WHERE {table} MATCH ?");
    let mut parameters = vec![Value::Text(expression)];
    bind(&mut sql, &mut parameters, probe);
    let mut hits = Vec::new();
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(params_from_iter(parameters))?;
    while let Some(row) = rows.next()? {
        budget.tick()?;
        hits.push(Hit { entity: row.get(0)?, attribute: row.get(1)?, value: row.get(2)?, rank: row.get::<_, f64>(3)?.max(0.0) });
    }
    if relation == TextRelation::Substring {
        let mut kept = Vec::with_capacity(hits.len());
        for hit in hits {
            if matches!(&*terms.term(hit.value)?, Term::Atom(Atom::String(value)) if value.to_lowercase().contains(&folded)) {
                kept.push(hit);
            }
        }
        hits = kept;
    }
    Ok(hits)
}

fn scan(connection: &Connection, probe: &Probe<'_>, folded: &str, budget: &Budget) -> Result<Vec<Hit>> {
    let mut sql = "SELECT p.t1, p.t2, p.t3, t.key FROM live_props p JOIN terms t ON t.id = p.t3 WHERE 1".to_string();
    let mut parameters = Vec::new();
    bind(&mut sql, &mut parameters, probe);
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(params_from_iter(parameters))?;
    let mut hits = Vec::new();
    while let Some(row) = rows.next()? {
        budget.tick()?;
        if let Key::Atom(Atom::String(value)) = decode_key(&row.get::<_, Vec<u8>>(3)?)?
            && value.to_lowercase().contains(folded)
        {
            hits.push(Hit { entity: row.get(0)?, attribute: row.get(1)?, value: row.get(2)?, rank: 0.0 });
        }
    }
    Ok(hits)
}

fn bind(sql: &mut String, parameters: &mut Vec<Value>, probe: &Probe<'_>) {
    for (column, id) in [("p.t1", probe.entity), ("p.t2", probe.attribute)] {
        if let Some(id) = id {
            sql.push_str(&format!(" AND {column} = ?"));
            parameters.push(Value::Integer(id));
        }
    }
}

/// The rows a text clause unifies against: entity, attribute, the needle's own id and, for text-search, the score.
pub fn rows(found: Vec<(i64, i64, f64)>, needle: i64, terms: &mut Terms<'_>, scored: bool) -> Result<Vec<Row>> {
    let mut rows = Vec::with_capacity(found.len());
    for (entity, attribute, score) in found {
        let mut row = vec![entity, attribute, needle];
        if scored {
            row.push(terms.id_of(&Term::float(score))?);
        }
        rows.push(Rc::from(row));
    }
    Ok(rows)
}

fn trigrams(folded: &str) -> String {
    let characters: Vec<char> = folded.chars().collect();
    let mut found: Vec<String> = Vec::new();
    for window in characters.windows(TRIGRAM) {
        let trigram = quoted(&window.iter().collect::<String>());
        if !found.contains(&trigram) {
            found.push(trigram);
        }
    }
    found.join(" AND ")
}
