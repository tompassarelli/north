use super::host_needs::of_with;
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
