use std::collections::BTreeSet;

use super::red_check;

const NOISE: [&str; 6] = ["aborting due to", "could not compile", "build failed", "test failed", "generated", "process didn't exit"];

/// The failures a check log names: failing tests and harness checks, and compiler or clippy diagnostics keyed by message and file, never by line, so a lane that shifts a finding down the file does not read as a new one.
pub fn findings(log: &str) -> BTreeSet<String> {
    let mut found: BTreeSet<String> = red_check::failures(log).iter().map(red_check::Failure::key).collect();
    let lines: Vec<&str> = log.lines().map(str::trim).collect();
    for (at, line) in lines.iter().enumerate() {
        let Some(message) = line.strip_prefix("error").or_else(|| line.strip_prefix("warning")).and_then(|rest| rest.split_once(": ")).map(|(_, message)| message) else { continue };
        if NOISE.iter().any(|noise| message.contains(noise)) {
            continue;
        }
        let place = lines[at + 1..].iter().take(3).find_map(|next| next.strip_prefix("--> ")).map_or("", |place| place.rsplitn(3, ':').last().unwrap_or(place));
        found.insert(format!("diagnostic {message} @ {place}"));
    }
    found
}

/// The findings of a lane's failing log that the base tree's failing log does not have.
pub fn added(lane: &BTreeSet<String>, base: &BTreeSet<String>) -> Vec<String> {
    lane.difference(base).cloned().collect()
}

pub fn encode(found: &BTreeSet<String>) -> String {
    found.iter().map(|finding| finding.replace('\n', " ")).collect::<Vec<_>>().join("\n")
}

pub fn decode(text: &str) -> BTreeSet<String> {
    text.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "warning: unused variable: `x`\n --> crates/a/src/lib.rs:10:5\nerror: this could be simplified\n   --> crates/a/src/lib.rs:20:9\nerror: could not compile `a` (lib) due to 1 previous error\n---- tests::old_red stdout ----\n";

    // KEEP(bug): on 2026-11-06 an inferred land check that main also failed let every lane through, so a lane could add new clippy errors to a repository whose main already had some (north#634)
    #[test]
    fn an_inferred_check_lets_a_lane_through_only_when_it_adds_no_finding_the_base_lacks() {
        let base = findings(BASE);
        assert!(base.contains("diagnostic this could be simplified @ crates/a/src/lib.rs") && base.contains("cargo tests::old_red"), "{base:?}");
        assert!(!base.iter().any(|finding| finding.contains("could not compile")), "compiler summary lines are not findings");

        let shifted = findings("error: this could be simplified\n   --> crates/a/src/lib.rs:42:9\n---- tests::old_red stdout ----\n");
        assert!(added(&shifted, &base).is_empty(), "the same finding moved down the file is not new");

        let worse = findings("error: this could be simplified\n   --> crates/a/src/lib.rs:20:9\nerror: new lint fired\n   --> crates/b/src/lib.rs:3:1\n");
        assert_eq!(added(&worse, &base), ["diagnostic new lint fired @ crates/b/src/lib.rs"]);

        assert_eq!(decode(&encode(&base)), base);
    }
}
