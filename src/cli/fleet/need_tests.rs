use super::*;

/// north#893: on 2026-11-08 n882-retain ran `north fleet need` with `--item` after the reason, as the usage line orders it, and the
/// need reached nexus without its item, because only `--item` straight after HOST was read and the rest folded into the reason.
#[test]
fn an_item_named_after_the_reason_is_forwarded_to_the_ledger_host() {
    let asked = Need::parse(&["laptop", "the", "window", "move", "--item", "tompassarelli/north#882"]).unwrap();

    assert_eq!(asked.forwarded("n882-retain"), ["north", "fleet", "dispatch", "need", "n882-retain", "laptop", "tompassarelli/north#882"]);
    assert_eq!(asked.reason, "the window move");
}
