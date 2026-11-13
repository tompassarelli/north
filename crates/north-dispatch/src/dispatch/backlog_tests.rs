use north_graph_ground::{Backlog, Entry, Held};

use super::decide;
use super::tests::{fleet, issue, survey};

fn entry(issue: &str, node: &str, text: &str) -> Entry {
    Entry { issue: issue.into(), node: node.into(), text: text.into() }
}

#[test]
fn dispatch_staffs_in_the_documents_rank_order_and_skips_each_box_that_waits_naming_the_edge() {
    let (hosts, accounts) = fleet();
    let mut boxes = vec![issue("north", 1, "- [ ] older", &[]), issue("north", 2, "- [ ] newer but ranked first", &[]), issue("north", 3, "- [ ] waits", &[])];
    for (created, issue) in [1.0, 2.0, 3.0].into_iter().zip(&mut boxes) {
        issue.created = created;
    }
    let mut ranked = survey(boxes, hosts, accounts);
    let held = Held { entry: entry("north#3", "box:3a", "waits"), edge: "north#2".into(), edge_text: "newer but ranked first".into() };
    ranked.backlog = Some(Backlog { ready: vec![entry("north#2", "box:2a", "a"), entry("north#1", "box:1a", "b")], held: vec![held], ranks: [("north#2".to_owned(), 1), ("north#1".to_owned(), 2)].into(), ..Backlog::default() });
    let outcome = decide(&ranked, "test");
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["north#2", "north#1"], "the ranked-first issue launches before the older one");
    let skipped: Vec<(String, &str)> = outcome.skipped.iter().map(|(item, why)| (item.to_string(), why.as_str())).collect();
    assert_eq!(skipped, [("north#3".to_owned(), "no box is ready: box \"waits\" waits on north#2 \"newer but ranked first\"")]);
    ranked.backlog = None;
    let plain: Vec<String> = decide(&ranked, "test").launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(plain, ["north#1", "north#2", "north#3"], "with no graph the issues keep their label and age order");
    let (hosts, accounts) = fleet();
    let mut split = survey(vec![issue("north", 4, "- [ ] build it\n- [ ] measure it", &[])], hosts, accounts);
    let waits = Held { entry: entry("north#4", "box:north#4:measure-it", "measure it"), edge: "box:north#4:build-it".into(), edge_text: "build it".into() };
    split.backlog = Some(Backlog { ready: vec![entry("north#4", "box:north#4:build-it", "build it")], held: vec![waits], ..Backlog::default() });
    let staffed: Vec<Option<String>> = decide(&split, "test").launches.iter().map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect();
    assert_eq!(staffed, [Some("build it".to_owned())], "a box the graph holds behind a box of its own issue is not staffed beside the ready one (north#1279's specialist box was)");
}
