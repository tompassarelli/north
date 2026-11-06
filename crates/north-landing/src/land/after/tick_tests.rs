use super::*;

const PLAN: &str = "## Done when\n- [x] W1 the sender is a required value\n- [ ] L1 `Item:` and `Box:` trailers, checked before the push\n- [ ] L2 the tick function\n- [ ] L3 `Next:` and retirement\n- [ ] (observed) a week of ticks re-measures each metric\n- [ ] (headless run) runtime promotion reaches every host\n- [ ] Tom tries it on his pad in his next playtest\n";

#[test]
fn an_issue_closes_only_when_every_box_is_ticked_and_a_landed_box_naming_its_own_done_when_command_is_left_to_that_command() {
    assert!(!closable(""));
    assert!(!closable("no boxes here\n"));
    assert!(!closable(PLAN));
    assert!(!closable("- [x] done\n- [ ] (observed) waits on a week\n"));
    assert!(closable("- [x] done\n  - [X] nested done\n* [x] star done\n"));
    let body = "- [ ] Parent\n  - [ ] Shares. Done when: `cargo test -p north-dispatch shares` passes.\n  - [ ] Prose. Done when the dry run shows it.\n";
    assert_eq!(tick(body, "Shares", Some("1a2b3c4d5e6f7")), Tick::NeedsChecker);
    let Tick::Ticked(edited) = tick(body, "Prose", Some("1a2b3c4d5e6f7")) else { panic!() };
    assert!(edited.contains("- [x] Prose. Done when the dry run shows it. (landed 1a2b3c4d5e6f)") && edited.starts_with("- [ ] Parent"), "{edited}");
    let stamped = body.replace("- [ ] Shares. Done when: `cargo test -p north-dispatch shares` passes.", "- [x] Shares. Done when: `cargo test -p north-dispatch shares` passes. Evidence: `cargo test -p north-dispatch shares` exited 0 on main 1a2b3c4d5e6f on 2026-11-06.");
    assert_eq!(tick(&stamped, "Shares", Some("1a2b3c4d5e6f7")), Tick::AlreadyTicked);
    let commanded_parent = "- [ ] Parent. Done when: `true` passes.\n  - [ ] Prose child\n";
    let Tick::Ticked(edited) = tick(commanded_parent, "Prose child", None) else { panic!() };
    assert!(edited.starts_with("- [ ] Parent."), "a parent with its own command waits for it: {edited}");
}
