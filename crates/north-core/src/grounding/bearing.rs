use super::*;
use compose::{HOOK_FLOOR, category, code, retrieve_db, shown};
use store::{Hit, Sqlite};

const NAMED: usize = 5;

pub fn records(paragraph: &str, now: f64) -> Vec<String> {
    records_at(paragraph, &store::db_path(), now)
}

pub fn records_at(paragraph: &str, index: &Path, now: f64) -> Vec<String> {
    let Ok(mut db) = Sqlite::open(index, true) else { return Vec::new() };
    records_in(paragraph, &mut db, now)
}

pub fn records_in(paragraph: &str, db: &mut Sqlite, now: f64) -> Vec<String> {
    let Ok((hits, _)) = retrieve_db(paragraph, db, now, 80, 0.0, "") else { return Vec::new() };
    let mut named: Vec<String> = Vec::new();
    for hit in hits.iter().filter(|hit| bears(hit)) {
        let place = shown(&hit.reference);
        if !named.contains(&place) {
            named.push(place);
        }
        if named.len() == NAMED {
            break;
        }
    }
    named
}

fn bears(hit: &Hit) -> bool {
    hit.cover >= HOOK_FLOOR && matches!(category(hit).as_str(), "Decisions" | "Policy") && !code(hit)
}

#[cfg(test)]
#[path = "bearing_tests.rs"]
mod tests;
