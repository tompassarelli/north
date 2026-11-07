//! `north ledger scoreboard --frontier`: north against plain Claude Code on the same boxes, read
//! from the stored run rows (docs/rationale/frontier.md).
use north_frontier::report::render;
use north_frontier::rows::{RESULTS, parse};

use crate::platform::harness;

pub(super) fn run() -> u8 {
    let read = harness::harness_root().and_then(|root| {
        let path = root.join(RESULTS);
        std::fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))
    });
    match read.and_then(|text| parse(&text)) {
        Ok(runs) => {
            print!("{}", render(&runs));
            0
        }
        Err(why) => {
            eprintln!("north ledger scoreboard: {why}");
            1
        }
    }
}
