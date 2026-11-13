use super::rows;
use crate::credit::{Balance, Grant, Reading, Standing};
use crate::usage::plain::grid;

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, granted: String::new(), expires: expires.to_owned() }
}

fn org(name: &str, expires: &str, spent: f64, later: &[Grant]) -> Standing {
    let left = 200.0 - spent + later.iter().map(|grant| grant.usd).sum::<f64>();
    Standing { name: name.to_owned(), names: vec![name.to_owned()], id: name.to_owned(), balance: Some(Balance { grant: grant(200.0, expires), spent, left: 200.0 - spent, later: later.to_vec() }), estimated: left, reading: None, left, exhausted: false, next_credit: None }
}

#[test]
fn the_api_section_is_one_aligned_table_in_the_order_the_pool_spends_it_with_its_provenance_printed_once() {
    let orgs = vec![
        org("zenith", "2026-12-08", 135.89, &[]),
        org("pine", "2026-11-30", 0.0, &[grant(100.0, "2027-01-02")]),
        org("spent", "2026-11-20", 200.0, &[]),
        org("ember", "2026-11-12", 21.8, &[]),
        Standing { balance: None, ..org("empty", "2026-11-12", 0.0, &[]) },
        Standing { exhausted: true, ..org("refused", "2026-11-15", 10.0, &[]) },
    ];
    let reading = Reading { hosts: vec![("ccx33".to_owned(), 59_000), ("here".to_owned(), 61_000)], ..Reading::default() };
    let peers = ["ccx33".to_owned(), "gex45".to_owned(), "here".to_owned()];
    let mut text = grid(&rows(orgs, true), 0);
    text.push(reading.footer("here", &peers, 100_000));
    assert_eq!(
        text.join("\n"),
        "\
API       grant     expires   spent     left        then
ember     $200.00   Nov 12    $21.80    $178.20
pine      $200.00   Nov 30    $0.00     $300.00     $100.00 Jan 2
zenith    $200.00   Dec 8     $135.89   $64.11
spent     $200.00   Nov 20    $200.00   $0.00
refused   $200.00   Nov 15    $10.00    exhausted
empty     none      —         —         —
API spend read on here and ccx33; oldest reading under a minute; no reading from gex45."
    );
}
