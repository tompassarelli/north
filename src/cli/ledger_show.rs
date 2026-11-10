//! `north ledger show L<id>`: one decision the distill ledger recorded, by the id a hook or a
//! report cites, so a citation can be opened instead of costing a turn.
use std::path::Path;

use north_core::grounding::distill::{self, Row};

const USAGE: &str = "usage: north ledger show L<id>";

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [flag] if flag == "-h" || flag == "--help") {
        println!("{USAGE}");
        return 0;
    }
    run_at(arguments, &distill::ledger_path())
}

fn run_at(arguments: &[String], ledger: &Path) -> u8 {
    let [word] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(id) = record_id(word) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match distill::show_at(id, ledger) {
        Ok(Some(row)) => {
            print!("{}", listing(&row));
            0
        }
        Ok(None) => {
            eprintln!("north ledger show: no decision {id}");
            1
        }
        Err(error) => {
            eprintln!("north ledger show: cannot read the decision ledger: {}", error.user_message());
            1
        }
    }
}

fn record_id(word: &str) -> Option<&str> {
    let id = word.strip_prefix("ledger:").unwrap_or(word);
    (id.starts_with('L') && id.len() > 1).then_some(id)
}

fn listing(row: &Row) -> String {
    format!("id: {}\ndate: {}\nowner: {}\nsource: {}\ndecision: {}\n", row.id, row.at, row.owner(), row.source, row.quote)
}

#[cfg(test)]
#[path = "ledger_show_tests.rs"]
mod tests;
