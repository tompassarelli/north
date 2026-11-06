use std::time::{Duration, Instant};

use crate::router::brief::CATEGORIES;

const GRAPH_BUDGET: Duration = Duration::from_millis(1500);

const INFRA: [&str; 1] = ["tooling"];
const BUG: [&str; 2] = ["bug-known-cause", "debugging-unknown-cause"];

pub fn skeleton(category: &str, neighborhood: &[String]) -> Option<String> {
    CATEGORIES.contains(&category).then(|| {
        let mut lines = vec![
            "Item: <repo#number and title>".to_owned(),
            "Box: <the text of the Done when box this lane closes, copied from the issue>".to_owned(),
            format!("Category: {category}"),
        ];
        if BUG.contains(&category) {
            lines.push("Class: <the class of failure, so the Done when covers every instance>".into());
        }
        lines.extend(
            [
                "Spec: exact",
                "Scope: module",
                "Surface: shell",
                "Verify: local-test",
                "Done when: `<command that proves it>`",
                "ETA: <minutes> min",
                "Files: <paths>",
            ]
            .map(str::to_owned),
        );
        if INFRA.contains(&category) {
            lines.push("Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days".into());
        }
        lines.push(String::new());
        lines.push("<Goal, Sources and the state checked at spawn>".into());
        if !neighborhood.is_empty() {
            lines.push(String::new());
            lines.push("Fact graph at spawn (a worker's Done claim is a claim until the checker observed its commit):".into());
            lines.extend(neighborhood.iter().cloned());
        }
        lines.push(String::new());
        lines.push("Build rule: run `cargo check` on the crates you touched plus the named tests in Done when, then queue the lane; the lander lands it in seconds, runs the full clippy and test suite once per landed batch afterwards, and reverts the lane and tells you if it fails, unless Done when names a full run.".into());
        lines.join("\n") + "\n"
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let (category, item) = match arguments {
        [new, flag, category] if new == "new" && flag == "--category" => (category, None),
        [new, flag, category, item_flag, item] if new == "new" && flag == "--category" && item_flag == "--item" => (category, Some(item)),
        _ => {
            eprintln!("usage: north brief new --category <{}> [--item REPO#N]", CATEGORIES.join("|"));
            return 2;
        }
    };
    let neighborhood = item.map(|item| north_graph_ground::ground(item, Some(Instant::now() + GRAPH_BUDGET))).unwrap_or_default();
    match skeleton(category, &neighborhood) {
        Some(text) => {
            print!("{text}");
            0
        }
        None => {
            eprintln!("north brief: unknown category {category}; use one of {}", CATEGORIES.join(" | "));
            2
        }
    }
}
