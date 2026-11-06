use crate::boxes::{self, Item};
use crate::waits::{self, Wait};

fn items(lines: &[&str]) -> Vec<Item> {
    boxes::parse(&lines.iter().map(|line| format!("- [ ] {line}\n")).collect::<String>())
}

fn of(lines: &[&str], index: usize) -> waits::Waits {
    let siblings = items(lines);
    waits::of(&siblings[index], "north#7", &siblings)
}

#[test]
fn the_wait_phrases_issue_bodies_use_are_read_as_edges_and_one_that_cannot_be_resolved_is_named_not_guessed() {
    let lines = ["B16 Capacity first", "B20 Then the move (after B16 and north#560)", "Fourth waits on box 1 and on #561"];
    let siblings = items(&lines);
    assert_eq!(of(&lines, 1).on, [Wait::Issue("north#560".into()), Wait::Box(siblings[0].id("north#7"))]);
    assert_eq!(of(&lines, 2).on, [Wait::Issue("north#561".into()), Wait::Box(siblings[0].id("north#7"))]);

    let lines = ["F1 first", "gated after R3 is on the runtime", "waits on the review-items-by-kind box landing", "(waits: 2026-11-12, a week of ticks) needs three days of rows", "owner moves after a restart; needs north#9", "ready after landing, 70-80% of runs finish in time", "F2 second (after F2, north#7)"];
    assert_eq!(of(&lines, 1).unparsed, ["after R3 is on the runtime"]);
    assert_eq!(of(&lines, 2).unparsed, ["waits on the review-items-by-kind box landing"]);
    assert_eq!(of(&lines, 4).on, [Wait::Issue("north#9".into())]);
    for quiet in [3, 5, 6] {
        let found = of(&lines, quiet);
        assert!(found.on.is_empty() && found.unparsed.is_empty(), "a date, a percentage range or a wait on itself is no edge: {found:?}");
    }

    let (named, unparsed) = waits::follows("Follows: north#12, #13\n- Follows: when the lander is back", "north");
    assert_eq!(named, ["north#12", "north#13"]);
    assert_eq!(unparsed, ["Follows: when the lander is back"]);
}
