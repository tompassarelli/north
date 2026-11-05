use super::*;

const PLAN: &str = "## Done when\n- [x] W1 the sender is a required value\n- [ ] L1 `Item:` and `Box:` trailers, checked before the push\n- [ ] L2 the tick function\n- [ ] L3 `Next:` and retirement\n- [ ] (observed) a week of ticks re-measures each metric\n- [ ] (headless run) runtime promotion reaches every host\n- [ ] Tom tries it on his pad in his next playtest\n";

#[test]
fn an_issue_closes_only_when_at_least_one_box_parsed_and_every_box_is_ticked() {
    assert!(!closable(""));
    assert!(!closable("no boxes here\n"));
    assert!(!closable(PLAN));
    assert!(!closable("- [x] done\n- [ ] (observed) waits on a week\n"));
    assert!(closable("- [x] done\n  - [X] nested done\n* [x] star done\n"));
}
