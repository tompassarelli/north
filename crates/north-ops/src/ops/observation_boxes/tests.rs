use super::*;

const BODY: &str = "Intro.\n- [ ] Plain box with no observation. Done when: it is.\n- [ ] The tick is fast. Observe: `true`\n- [ ] The reader is fed. Observe: `exit 3`\n- [x] Already done. Observe: `true`\n";

#[test]
fn only_unticked_boxes_that_name_a_command_are_observations() {
    let found = observations(BODY);
    assert_eq!(found.iter().map(|o| (o.line, o.command.as_str())).collect::<Vec<_>>(), vec![(2, "true"), (3, "exit 3")]);
}

#[test]
fn a_command_exiting_zero_ticks_its_box_with_evidence_and_a_nonzero_one_stays_open() {
    let (edited, observed) = settle_body(BODY, &observe, "2026-11-05");
    assert_eq!(observed.iter().map(|run| run.exit).collect::<Vec<_>>(), vec![0, 3]);
    let lines: Vec<&str> = edited.lines().collect();
    assert_eq!(lines[1], "- [ ] Plain box with no observation. Done when: it is.");
    assert_eq!(lines[2], "- [x] The tick is fast. Observe: `true` Evidence: `true` exited 0 on 2026-11-05.");
    assert_eq!(lines[3], "- [ ] The reader is fed. Observe: `exit 3`");
    assert_eq!(lines[4], "- [x] Already done. Observe: `true`");
    assert!(edited.ends_with('\n'));
}

#[test]
fn a_missing_command_is_an_open_box() {
    assert_ne!(observe("no-such-observation-command-north522"), 0);
}

#[test]
fn the_verb_takes_one_source_and_refuses_the_rest() {
    let words = |list: &[&str]| list.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    assert!(options(&words(&["--observations", "--dry-run", "north#522"])).unwrap().dry_run);
    assert!(options(&words(&["--observations"])).is_err());
    assert!(options(&words(&["--observations", "north#1", "--repo", "north"])).is_err());
}
