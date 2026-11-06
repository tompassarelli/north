use std::collections::BTreeMap;

use super::{budget_key, family, key, over, samples, slow};

fn span(id: &str, parent: &str, name: &str, start: f64, end: f64, exit: Option<&str>) -> String {
    let attributes = exit.map_or_else(String::new, |exit| format!(",\"attributes\":{{\"exit\":\"{exit}\"}}"));
    format!("{{\"trace\":\"t\",\"id\":\"{id}\",\"parent\":\"{parent}\",\"name\":\"{name}\",\"category\":\"check\",\"start\":{start},\"end\":{end}{attributes}}}\n")
}

#[test]
fn a_family_over_its_budget_in_each_newest_green_run_files_one_box_counting_only_its_own_time() {
    let clippy = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets";
    let docs = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs docs rationale-index --check";
    let mut log = String::new();
    for (run, own) in [(0.0, 70.0), (1000.0, 80.0), (2000.0, 90.0)] {
        let root = format!("r{run}");
        log.push_str(&format!("{{\"trace\":\"t\",\"id\":\"{root}\",\"name\":\"post-land checks\",\"category\":\"lander\",\"start\":{run},\"end\":{}}}\n", run + 200.0));
        log.push_str(&span(&format!("c{run}"), &root, clippy, run, run + own, Some("0")));
        log.push_str(&span(&format!("d{run}"), &root, docs, run, run + 40.0, Some("0")));
        log.push_str(&span(&format!("w{run}"), &format!("d{run}"), "offload slot wait", run, run + 38.0, None));
    }
    log.push_str(&span("red", "r0", clippy, 3000.0, 3001.0, Some("101")));
    let found = samples(&log);
    assert_eq!(found.iter().filter(|sample| sample.family == "head_north").map(|sample| sample.seconds.round()).collect::<Vec<_>>(), [2.0, 2.0, 2.0]);
    let budgets = BTreeMap::from([("clippy", 65.0), ("head_north", 10.0)]);
    let overs = over(&found, &budgets, 3);
    assert_eq!(overs.iter().map(|over| (over.family, over.newest.len())).collect::<Vec<_>>(), [("clippy", 3)]);
    assert!(over(&found, &BTreeMap::from([("clippy", 75.0)]), 3).is_empty(), "one run at or under budget clears the streak");
    let findings = slow(&overs, "north#634");
    assert_eq!((findings.len(), findings[0].key.as_str(), findings[0].owner.as_deref()), (1, key("clippy").as_str(), Some("north#634")));
    assert!(findings[0].detail.contains("70 s, 80 s, 90 s") && findings[0].detail.contains(&budget_key("clippy")), "{}", findings[0].detail);
    assert_eq!([family("x ./bin/head-north --code a/ --build"), family("x ./bin/head-north land-tests --only portable"), family("x cargo nextest run")], [Some("north_build"), Some("land_tests"), Some("nextest")]);
}
