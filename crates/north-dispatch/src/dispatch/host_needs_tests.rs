use super::host_needs::{of_box_with, of_with, record, relay};
use super::issues::{Issue, IssueRef};
use super::tests::issue;
use super::tier::{Route, Tier};
use crate::pacing::placement::gate;

#[test]
fn a_lander_state_box_is_placed_on_the_lander_host() {
    let at = 1_794_270_600.0;
    let lander = |_: &IssueRef| Some("whiterabbit".to_owned());
    let land = issue("north", 980, "- [ ] No failed test of a main Cargo run since the last landing is left out. Done when: `north land missed --since 2026-11-09T00:30:00Z` exits 0", &[]);
    assert_eq!(of_with(&land, &[], at, &lander), ["whiterabbit"]);
    assert_eq!(of_with(&land, &[], at, &|_| None), ["lander"], "a repo whose lander host is unknown keeps the need, so no host is picked silently");

    let loops = issue("north", 987, "- [ ] The loop passes on the lander. Done when: `north loops check --role lander 2>&1 | rg 'main-red: ok'` prints a line", &[]);
    assert_eq!(of_with(&loops, &[], at, &lander), ["whiterabbit"]);
    let code = issue("north", 990, "- [ ] Make `north land missed` refuse a stale mirror. Done when: `cargo nextest run -p north-land-checks -E 'test(/a_stale_lander_mirror_is_refused_not_passed/)'` exits 0", &[]);
    assert!(of_with(&code, &[], at, &lander).is_empty(), "a code-only check stays off the lander, even when its prose names a lander command");

    let route = Route { tier: Tier::Opus, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let brief = super::brief::brief(&Issue { needs: vec!["whiterabbit".into()], ..land }, "dispatch", &route, &super::grounds::Grounds::default(), None);
    assert!(brief.lines().any(|line| line == "Needs: whiterabbit"), "{brief}");
    let cfg: toml_edit::DocumentMut = "[hosts.gex45]\nlaunches = true\nserves = []\n[hosts.whiterabbit]\nlaunches = true\nserves = [\"lander\"]\n".parse().unwrap();
    assert_eq!(gate::lacks(&cfg, "gex45", &brief).as_deref(), Some("reach to whiterabbit"));
    assert_eq!(gate::lacks(&cfg, "whiterabbit", &brief), None);
}

#[test]
fn a_box_that_ends_its_needs_line_with_a_full_stop_is_placed_on_the_host_that_serves_it() {
    let at = 1_794_270_600.0;
    let lander = |_: &IssueRef| None;
    let boxed = issue("north", 1048, "- [ ] On the laptop, by hand: `north fleet revive proxy` on a stopped proxy leaves a niri window. Needs: laptop. Done when: `niri msg -j windows | jq -e 'any(.[]; .title == \"north proxy\")'` exits 0", &[]);
    let needs = of_with(&boxed, &[], at, &lander);
    assert_eq!(needs, ["laptop"]);
    let route = Route { tier: Tier::Opus, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let brief = super::brief::brief(&Issue { needs, ..boxed }, "dispatch", &route, &super::grounds::Grounds::default(), None);
    let cfg: toml_edit::DocumentMut = "[hosts.cpx62b]\nlaunches = true\nserves = []\n[hosts.whiterabbit]\nlaunches = true\nserves = [\"laptop\"]\n".parse().unwrap();
    assert!(gate::lacks(&cfg, "cpx62b", &brief).is_some(), "{brief}");
    assert_eq!(gate::lacks(&cfg, "whiterabbit", &brief), None);
}

#[test]
fn a_need_recorded_for_one_box_holds_no_other_box() {
    let at = 1_794_270_600.0;
    let lander = |_: &IssueRef| None;
    let body = "- [ ] On the laptop, by hand: the window opens. Needs: laptop. Done when: `niri msg windows` exits 0\n- [ ] Make the parser refuse an empty line. Done when: `cargo nextest run -p north-dispatch -E 'test(/an_empty_line_is_refused/)'` exits 0";
    let tracked = issue("north", 1022, body, &[]);
    let directory = tempfile::tempdir().unwrap();
    let learned = record(directory.path(), &tracked, Some("On the laptop, by hand"), "n1022-laptop", "laptop", "the window needs the laptop", at).unwrap();
    assert_eq!(of_box_with(&tracked, "On the laptop, by hand", &learned, at, &lander), ["laptop"]);
    assert!(of_box_with(&tracked, "Make the parser refuse an empty line", &learned, at, &lander).is_empty(), "the code-only box is placed on any build host");

    let unnamed = record(directory.path(), &tracked, None, "n1022-unnamed", "native", "a need from a launch that named no box", at).unwrap();
    assert!(of_box_with(&tracked, "Make the parser refuse an empty line", &unnamed, at, &lander).is_empty(), "a launch that named no box holds no other box");
    assert_eq!(relay(&unnamed, "north#1022", "n1022-unnamed").map(|relay| relay.need).as_deref(), Some("native"), "its record still follows its own next try");
}

#[test]
fn a_dispatched_brief_for_one_box_carries_no_need_recorded_for_another_box() {
    let at = 1_794_270_600.0;
    let body = "- [ ] On the laptop, by hand: the window opens. Needs: laptop. Done when: `niri msg windows` exits 0\n- [ ] Make the parser refuse an empty line. Done when: `cargo nextest run -p north-dispatch -E 'test(/an_empty_line_is_refused/)'` exits 0";
    let tracked = issue("north", 1022, body, &[]);
    let learned = record(tempfile::tempdir().unwrap().path(), &tracked, Some("On the laptop, by hand"), "n1022-laptop", "laptop", "the window needs the laptop", at).unwrap();
    let survey = super::survey::Survey { learned, at, ..super::tests::survey(vec![tracked.clone()], Vec::new(), Vec::new()) };
    let needs_line = |box_text: &str| super::screen::written(&survey, &tracked, "dispatch", Some(box_text)).unwrap().1.lines().find(|line| line.starts_with("Needs:")).map(str::to_owned);
    assert_eq!(needs_line("On the laptop, by hand").as_deref(), Some("Needs: laptop"));
    assert_eq!(needs_line("Make the parser refuse an empty line"), None);
}
