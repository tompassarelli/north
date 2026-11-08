use std::collections::HashMap;

use rusqlite::{Connection, params_from_iter};

use super::Field;
use crate::error::Result;
use crate::store::Graph;
use crate::store::resolve::Terms;
use crate::term::Term;

const VOCABULARY: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS temp.text_stems_rows USING fts5vocab(main, text_stems, row);
CREATE VIRTUAL TABLE IF NOT EXISTS temp.text_stems_instances USING fts5vocab(main, text_stems, instance);
CREATE VIRTUAL TABLE IF NOT EXISTS temp.text_tokens USING fts5(word, tokenize='porter unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE IF NOT EXISTS temp.text_tokens_instances USING fts5vocab(temp, text_tokens, instance);
";

/// A ranked search of the stemmed index: `query` is an FTS5 expression, `weights` the bm25 weights of the name and
/// body columns, and each hit carries the objects of `facets` on its subject. With facets, a hit's subject must carry
/// the first one, and `cap` keeps at most that many hits per value of it.
pub struct TextSearch<'a> {
    pub query: &'a str,
    pub field: Option<Field>,
    pub weights: [f64; 2],
    pub facets: &'a [&'a str],
    pub cap: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextHit {
    pub subject: Term,
    pub facets: Vec<Option<Term>>,
    pub rank: f64,
}

impl Graph {
    pub fn text_rows(&self) -> Result<u64> {
        Ok(self.connection().query_row("SELECT count(*) FROM text_stems_docsize", [], |row| row.get::<_, i64>(0))?.unsigned_abs())
    }

    pub fn text_matches(&self, query: &str) -> Result<u64> {
        Ok(self.connection().query_row("SELECT count(*) FROM text_stems WHERE text_stems MATCH ?1", [query], |row| row.get::<_, i64>(0))?.unsigned_abs())
    }

    pub fn text_search(&self, search: &TextSearch) -> Result<Vec<TextHit>> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let mut facets = Vec::with_capacity(search.facets.len());
        for facet in search.facets {
            facets.push(terms.stored_id_of(&Term::keyword(*facet))?);
        }
        if facets.first().is_some_and(Option::is_none) {
            return Ok(Vec::new());
        }
        let query = match search.field {
            Some(Field::Name) => format!("name : ({})", search.query),
            Some(Field::Body) => format!("body : ({})", search.query),
            None => search.query.to_string(),
        };
        let sql = search_sql(facets.len(), search.cap.is_some());
        let mut values: Vec<rusqlite::types::Value> = vec![search.weights[0].into(), search.weights[1].into(), query.into()];
        values.extend(facets.iter().map(|id| id.unwrap_or(0).into()));
        if let Some(cap) = search.cap {
            values.push(i64::try_from(cap).unwrap_or(i64::MAX).into());
        }
        let mut statement = snapshot.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            let mut ids = Vec::with_capacity(facets.len());
            for slot in 0..facets.len() {
                ids.push(row.get::<_, Option<i64>>(slot + 2)?);
            }
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, ids))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (subject, rank, ids) = row?;
            let mut values = Vec::with_capacity(ids.len());
            for id in ids {
                values.push(match id {
                    Some(id) => Some(terms.term(id)?.as_ref().clone()),
                    None => None,
                });
            }
            hits.push(TextHit { subject: terms.term(subject)?.as_ref().clone(), facets: values, rank });
        }
        Ok(hits)
    }

    pub fn text_stems(&self, words: &[&str]) -> Result<Vec<Option<String>>> {
        let connection = self.connection();
        connection.execute_batch(VOCABULARY)?;
        connection.execute("DELETE FROM temp.text_tokens", [])?;
        for (row, word) in words.iter().enumerate() {
            connection.prepare_cached("INSERT INTO temp.text_tokens (rowid, word) VALUES (?1, ?2)")?.execute(rusqlite::params![i64::try_from(row).unwrap_or(i64::MAX), word])?;
        }
        let mut stems = vec![None; words.len()];
        let mut statement = connection.prepare_cached("SELECT doc, term FROM temp.text_tokens_instances ORDER BY doc, offset")?;
        for row in statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))? {
            let (row, term) = row?;
            if let Some(slot) = usize::try_from(row).ok().and_then(|row| stems.get_mut(row)) {
                slot.get_or_insert(term);
            }
        }
        Ok(stems)
    }

    pub fn text_frequencies(&self, stems: &[&str]) -> Result<HashMap<String, u64>> {
        let connection = vocabulary(self.connection())?;
        let mut statement = connection.prepare_cached("SELECT doc FROM temp.text_stems_rows WHERE term = ?1")?;
        let mut found = HashMap::new();
        for stem in stems {
            let mut rows = statement.query([stem])?;
            if let Some(row) = rows.next()? {
                found.insert((*stem).to_string(), row.get::<_, i64>(0)?.unsigned_abs());
            }
        }
        Ok(found)
    }

    /// For each stem, how many indexed subjects hold it per object of their `facet`.
    pub fn text_frequencies_by(&self, stems: &[&str], facet: &str) -> Result<Vec<(String, Term, u64)>> {
        let connection = vocabulary(self.connection())?;
        let mut terms = Terms::new(connection);
        let Some(facet) = terms.stored_id_of(&Term::keyword(facet))? else { return Ok(Vec::new()) };
        let mut statement = connection.prepare_cached(
            "SELECT p.t3, count(DISTINCT v.doc) FROM temp.text_stems_instances v CROSS JOIN live_props p ON p.t1 = v.doc AND p.t2 = ?2 WHERE v.term = ?1 GROUP BY p.t3",
        )?;
        let mut found = Vec::new();
        for stem in stems {
            for row in statement.query_map(rusqlite::params![stem, facet], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?.unsigned_abs())))? {
                let (value, count) = row?;
                found.push(((*stem).to_string(), terms.term(value)?.as_ref().clone(), count));
            }
        }
        Ok(found)
    }

    /// Indexed stems within `slack` characters of `stem`'s length that share its first or last two characters, the
    /// most widely held first, with the number of subjects holding each.
    pub fn text_spelling_candidates(&self, stem: &str, slack: usize, limit: usize) -> Result<Vec<(String, u64)>> {
        let connection = vocabulary(self.connection())?;
        let chars: Vec<char> = stem.chars().collect();
        let head: String = chars.iter().take(2).collect();
        let tail: String = chars[chars.len().saturating_sub(2)..].iter().collect();
        let length = i64::try_from(chars.len()).unwrap_or(i64::MAX);
        let slack = i64::try_from(slack).unwrap_or(i64::MAX);
        let mut statement = connection.prepare_cached(
            "SELECT term, doc FROM temp.text_stems_rows WHERE length(term) BETWEEN ?1 AND ?2 AND (substr(term, 1, 2) = ?3 OR substr(term, -2) = ?4) ORDER BY doc DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            rusqlite::params![length.saturating_sub(slack).max(0), length.saturating_add(slack), head, tail, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?.unsigned_abs())),
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn vocabulary(connection: &Connection) -> Result<&Connection> {
    connection.execute_batch(VOCABULARY)?;
    Ok(connection)
}

fn search_sql(facets: usize, capped: bool) -> String {
    let mut columns = vec!["text_stems.rowid AS c0".to_string(), "bm25(text_stems, ?1, ?2) AS c1".to_string()];
    let mut joins = String::new();
    for slot in 0..facets {
        let parameter = slot + 4;
        let column = slot + 2;
        if slot == 0 {
            joins.push_str(&format!(" JOIN live_props f0 ON f0.t1 = text_stems.rowid AND f0.t2 = ?{parameter}"));
            columns.push(format!("f0.t3 AS c{column}"));
        } else {
            columns.push(format!("(SELECT f.t3 FROM live_props f WHERE f.t1 = text_stems.rowid AND f.t2 = ?{parameter} ORDER BY f.prop LIMIT 1) AS c{column}"));
        }
    }
    let scan = format!("SELECT {} FROM text_stems{joins} WHERE text_stems MATCH ?3", columns.join(", "));
    if capped && facets > 0 {
        let names: Vec<String> = (0..columns.len()).map(|column| format!("c{column}")).collect();
        format!(
            "WITH scored AS MATERIALIZED ({scan}) SELECT {names} FROM (SELECT {names}, row_number() OVER (PARTITION BY c2 ORDER BY c1) AS place FROM scored) WHERE place <= ?{cap} ORDER BY c1",
            names = names.join(", "),
            cap = facets + 4
        )
    } else {
        format!("{scan} ORDER BY c1")
    }
}
