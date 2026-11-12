use super::*;

const PLAN: &str = "## Done when\n- [x] W1 the sender is a required value\n- [ ] L1 `Item:` and `Box:` trailers, checked before the push\n- [ ] L2 the tick function\n- [ ] L3 `Next:` and retirement\n- [ ] (observed) a week of ticks re-measures each metric\n- [ ] (headless run) runtime promotion reaches every host\n- [ ] Tom tries it on his pad in his next playtest\n";

#[test]
fn an_issue_closes_only_when_every_box_is_ticked_and_a_landed_box_naming_its_own_done_when_command_is_left_to_that_command() {
    assert!(!closable(""));
    assert!(!closable("no boxes here\n"));
    assert!(!closable(PLAN));
    assert!(!closable("- [x] done\n- [ ] (observed) waits on a week\n"));
    assert!(closable("- [x] done. Evidence: `true` exited 0 on main 1a2b3c4d5e6f on 2026-11-06.\n  - [X] nested, split to north#900\n* [x] star done. Evidence: `true` passed on 2026-11-06: ok\n"));
    let landed = "- [x] Ship it. Evidence: `true` exited 0 on main 1a2b3c4d5e6f on 2026-11-06.\n\n**Clean-up before closing**\n- [ ] Clean-up: scaffolding removed.\n";
    assert!(closable(landed), "a retired Clean-up line keeps no landed issue open");
    assert!(!closable(&landed.replace("\n\n", "\n- [ ] Wire it into dispatch\n\n")), "a real box still open keeps it open");
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

#[test]
fn a_sheriff_revert_of_a_landing_that_ticked_a_box_leaves_the_box_unticked_with_no_landed_stamp() {
    let body = "## Done when\n- [x] Fix the box. Evidence: `true` passed. (landed 1a2b3c4d5e6f)\n- [ ] Another box.\n";
    let change = Edit::Untick { box_text: "Fix the box".into(), sha: "1a2b3c4d5e6f7".into() }.apply(body);
    assert_eq!(change, crate::issue_body::Change::Applied("## Done when\n- [ ] Fix the box. Evidence: `true` passed.\n- [ ] Another box.\n".into()));
}

#[test]
fn a_sheriff_revert_plan_reads_the_boxes_of_the_landing_it_reverts() {
    let directory = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| {
        let output = Command::new("git").arg("-C").arg(directory.path()).args(["-c", "user.name=a", "-c", "user.email=a@b"]).args(arguments).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    run(&["init", "-q"]);
    std::fs::write(directory.path().join("a"), "a").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "base"]);
    std::fs::write(directory.path().join("b"), "b").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "Land the box (north#872)\n\nItem: north#872\nBox: Fix the box"]);
    let landed = run(&["rev-parse", "HEAD"]);
    std::fs::write(directory.path().join("c"), "c").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", &format!("Revert the box\n\nSheriff-Reverts: {landed}")]);

    assert_eq!(reverts(directory.path(), "HEAD~2", "HEAD").unwrap(), [Reverted { issue: Issue::parse("north#872").unwrap(), box_text: "Fix the box".into(), sha: landed }]);
}

#[test]
fn an_issue_whose_ticked_box_carries_no_passing_oracle_run_stays_open_as_north_766_should_have() {
    let n766 = "## Done when\n\n- [x] Children a lead staffs with `--task north#757` start and leave the lead holding north#757. Test `children_a_lead_staffs_on_boxes_of_its_own_issue_start_and_leave_the_lead_holding_it` replays gex45-n757-gates; landed as c72a68a36 on origin/main.\n";
    assert!(!closable(n766));
    let proven = "## Done when\n\n- [x] Claims. Done when: `cargo test -p north-fleet claims` passes. Evidence: `cargo test -p north-fleet claims` exited 0 on main c72a68a36 on 2026-11-07.\n- [x] Per-box claims, split to north#900 which owns them.\n- [x] Landed on origin/main.\n";
    assert!(closable(proven));
}
