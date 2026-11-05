#![cfg(test)]

use super::build_offload::*;

// KEEP(contract): a lane with a private game asset (.W3X, .blp) never ships to an offload host, and an oversized diff stays local; mutant bo-private-ships
#[test]
fn capacity_a_lane_with_a_private_asset_or_an_oversized_diff_stays_on_this_host() {
    assert_eq!(ships(&["src/lib.rs", "Cargo.toml"], 1000), Ok(()));
    let private = ships(&["maps/arena.W3X"], 10).unwrap_err();
    assert_eq!((private.reason(), private.detail()), ("refused: private asset".to_owned(), "maps/arena.W3X".to_owned()));
    assert!(ships(&["assets/a.blp"], 10).is_err());
    let oversized = ships(&["src/lib.rs"], MAX_DIFF_BYTES + 1).unwrap_err();
    assert_eq!(oversized.reason(), "refused: oversized diff");
    assert!(oversized.detail().contains("limit"));
}
