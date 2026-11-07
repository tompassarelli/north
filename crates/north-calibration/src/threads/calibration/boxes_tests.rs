use super::boxes::{BoxRun, box_name};
use super::read::chained;

fn run(agent: &str, brief: &str, staffed: &str, ended: &str) -> BoxRun {
    BoxRun { agent: agent.into(), item: "north#360".into(), outcome: "done".into(), staffed: Some(staffed.into()), ended: ended.into(), box_name: box_name(brief), ..BoxRun::default() }
}

#[test]
fn a_run_on_another_box_of_the_same_issue_is_no_reattempt_and_leaves_the_earlier_run_kept() {
    let sheriff = "Item: north#360 sheriff box: the main-red sheriff must re-verify before reverting\nCategory: bug-known-cause";
    let land_check = "Item: north#360 (North landing flow), land-check speed: cut the Actions land check to p50 under 90 s\nCategory: performance";
    let boxed = "Item: north#360 North landing flow\nBox: The sheriff re-verifies before reverting\nCategory: bug-known-cause";
    let rows = [
        run("sheriff-1", sheriff, "2026-11-03T23:45:59Z", "2026-11-04T01:00:34Z"),
        run("land-check-1", land_check, "2026-11-04T00:46:06Z", "2026-11-04T00:59:16Z"),
        run("sheriff-2", sheriff, "2026-11-04T02:10:00Z", "2026-11-04T02:40:00Z"),
        run("boxed-1", boxed, "2026-11-04T03:00:00Z", "2026-11-04T03:20:00Z"),
    ];
    let attempts = chained(&rows);
    let number = |agent: &str| attempts.numbers[agent];
    assert_eq!([number("sheriff-1"), number("land-check-1"), number("sheriff-2"), number("boxed-1")], [1, 1, 2, 1], "only the same box run again is a second attempt");
    assert_eq!(attempts.reruns.keys().map(String::as_str).collect::<Vec<_>>(), ["sheriff-1"], "the land-check run is kept though a sheriff run followed it on the same issue");
    assert_eq!(box_name(boxed), "The sheriff re-verifies before reverting", "a Box: line names the box over the Item line");
    assert_eq!(box_name(""), "", "a run whose brief is unread chains per issue, as before");
}
