use super::Currency;
use crate::land::candidate::git;
use crate::land::queue::Ticket;

// KEEP(bug): north#892 review on 2026-11-08: an alone pass that named only the lane head never expired, so a pass checked on an hour-old main still vouched for the lane, and a land-first lander would land it untested on the main it reaches
#[test]
fn an_alone_pass_vouches_for_its_lane_until_main_gains_more_landings_than_the_setting_allows_or_stops_descending_from_the_main_it_was_checked_on() {
    let repo = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| git(repo.path(), arguments).unwrap();
    run(&["init", "-q", "-b", "main"]);
    let commit = |message: &str| {
        run(&["-c", "user.name=lander", "-c", "user.email=lander@example.invalid", "commit", "-q", "--allow-empty", "-m", message]);
        run(&["rev-parse", "HEAD"])
    };
    let landed: Vec<String> = (0..=6).map(|landing| commit(&format!("landing {landing}"))).collect();
    run(&["checkout", "-q", "--orphan", "elsewhere"]);
    let elsewhere = commit("a main that does not descend from the pass");
    let ticket = Ticket { head: "lane".into(), alone_green: Some("lane".into()), alone_base: Some(landed[0].clone()), ..Ticket::default() };
    let pass = ticket.passed_alone_on().unwrap();
    assert!(Currency::at(repo.path(), &landed[5]).current(pass), "five landings past its pass the lane is still vouched for");
    assert!(!Currency::at(repo.path(), &landed[6]).current(pass), "six landings past its pass the lane must be checked alone again");
    assert!(!Currency::at(repo.path(), &elsewhere).current(pass), "a main that does not descend from the pass expires it");
    let moved = Ticket { head: "lane-rebased".into(), ..ticket };
    assert_eq!(moved.passed_alone_on(), None, "a pass for an older head of the lane vouches for nothing");
}
