use super::{Entry, rows};
use crate::credit::{Balance, Grant, Reading};
use crate::usage::plain::grid;

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, expires: expires.to_owned() }
}

fn entry(name: &str, expires: &str, spent: f64, later: &[Grant]) -> Entry {
    Entry { name: name.to_owned(), balance: Some(Balance { grant: grant(200.0, expires), spent, left: 200.0 - spent, later: later.to_vec() }), marked: false }
}

#[test]
fn the_api_section_is_one_aligned_table_in_the_order_the_pool_spends_it_with_its_provenance_printed_once() {
    let entries = vec![
        entry("proton", "2026-12-08", 135.89, &[]),
        entry("pmme", "2026-11-30", 0.0, &[grant(100.0, "2027-01-02")]),
        entry("spent", "2026-11-20", 200.0, &[]),
        entry("gmail", "2026-11-12", 21.8, &[]),
        Entry { name: "empty".to_owned(), balance: None, marked: false },
        Entry { marked: true, ..entry("refused", "2026-11-15", 10.0, &[]) },
    ];
    let reading = Reading { hosts: vec![("ccx33".to_owned(), 59_000), ("here".to_owned(), 61_000)], ..Reading::default() };
    let peers = ["ccx33".to_owned(), "gex45".to_owned(), "here".to_owned()];
    let mut text = grid(&rows(entries, true), 0);
    text.push(reading.footer("here", &peers, 100_000));
    assert_eq!(
        text.join("\n"),
        "\
API       grant     expires   spent     left        then
gmail     $200.00   Nov 12    $21.80    $178.20
pmme      $200.00   Nov 30    $0.00     $200.00     $100.00 Jan 2
proton    $200.00   Dec 8     $135.89   $64.11
refused   $200.00   Nov 15    $10.00    exhausted
spent     $200.00   Nov 20    $200.00   exhausted
empty     none      —         —         —
API spend read on here and ccx33; oldest reading under a minute; no reading from gex45."
    );
}
