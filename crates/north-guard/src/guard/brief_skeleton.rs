use std::process::Command;
use std::time::{Duration, Instant};

use north_base::platform::push_ledger;
use north_base::platform::records_mirror::state_home;
use north_base::platform::verification_budget;

use crate::router::brief::CATEGORIES;

const GRAPH_BUDGET: Duration = Duration::from_millis(1500);

const INFRA: [&str; 1] = ["tooling"];
const BUG: [&str; 2] = ["bug-known-cause", "debugging-unknown-cause"];

pub fn skeleton(category: &str, neighborhood: &[String], over_budget: bool) -> Option<String> {
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
            lines.push("Cause: <known: name the file or step | unknown: stop and relaunch with Category: debugging-unknown-cause, or performance for a slowness>".into());
            lines.push("Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days".into());
        }
        lines.push(String::new());
        lines.push("<Goal, Sources and the state checked at spawn>".into());
        if !neighborhood.is_empty() {
            lines.push(String::new());
            lines.push("Fact graph at spawn (a worker's Done claim is a claim until the checker observed its commit):".into());
            lines.extend(neighborhood.iter().cloned());
        }
        if over_budget {
            lines.push(String::new());
            lines.push("Test bar: verification is over its time budget, so this lane lands no net-new test; a test it adds is paid for by deleting one.".into());
        }
        lines.push(String::new());
        lines.push("Build rule: on a build host, lint and test the touched crate from the lane through `./bin/land-shell --dev`, so the lane grows no Cargo target: `./bin/land-shell --dev -- buck2 --isolation-dir north-g$(cat /etc/buildbarn/store-generation)-lint bxl --remote-only --materializations=none //lint/clippy.bxl:clippy -- --targets //crates/<crate>:` and `./bin/land-shell --dev -- buck2 --isolation-dir north-g$(cat /etc/buildbarn/store-generation) test --remote-only --unstable-allow-compatible-tests-on-re //crates/<crate>:`; elsewhere run `cargo clippy --all-targets -p <touched crates> -- -D warnings` plus the named tests in Done when, then queue the lane; the lander lands it in seconds, runs the full suite once per landed batch afterwards, and reverts the lane and tells you if it fails, unless Done when names a full run.".into());
        lines.join("\n") + "\n"
    })
}

fn repository(item: Option<&str>) -> String {
    if let Some((repository, _)) = item.and_then(|item| item.split_once('#')) {
        return repository.to_owned();
    }
    let origin = Command::new("git").args(["remote", "get-url", "origin"]).output().map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    push_ledger::repository(origin.trim())
}

pub fn run(arguments: &[String]) -> u8 {
    let (category, item) = match arguments {
        [new, flag, category] if new == "new" && flag == "--category" => (category, None),
        [new, flag, category, item_flag, item] if new == "new" && flag == "--category" && item_flag == "--item" => (category, Some(item)),
        _ => {
            eprintln!("usage: north work brief new --category <{}> [--item REPO#N]", CATEGORIES.join("|"));
            return 2;
        }
    };
    let neighborhood = item.map(|item| north_graph_ground::ground(item, Some(Instant::now() + GRAPH_BUDGET))).unwrap_or_default();
    let over_budget = verification_budget::is_over(&state_home(), &repository(item.map(String::as_str)));
    match skeleton(category, &neighborhood, over_budget) {
        Some(text) => {
            print!("{text}");
            0
        }
        None => {
            eprintln!("north work brief: unknown category {category}; use one of {}", CATEGORIES.join(" | "));
            2
        }
    }
}
