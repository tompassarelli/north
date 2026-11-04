use std::path::Path;

use super::queue::{self, Ticket};
use super::{records, telemetry};

pub const LANDER: &str = "lander";

pub struct Noticed<'a> {
    pub class: &'a str,
    pub key: String,
    pub noticed_by: &'a str,
    pub detail: &'a str,
}

pub fn class(verdict: &str) -> Option<&'static str> {
    if verdict.contains("deadline") || verdict.contains(" hung") {
        return Some("lander-hang");
    }
    if verdict.contains("persistent transient") || verdict.contains("persistent deferred") {
        return Some("lander-transient");
    }
    match verdict.split_whitespace().next() {
        Some("failed") => Some("lander-refused"),
        Some("conflict") => Some("lander-conflict"),
        Some("transient") => Some("lander-transient"),
        Some("deferred") => Some("lander-deferred"),
        Some("withdrawn") => Some("lander-withdrawn"),
        _ => None,
    }
}

fn name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A ticket the lander decided other than landed, keyed by its ticket file so a replayed decision adds no row.
pub fn decided<'a>(path: &Path, ticket: &Ticket, verdict: &'a str) -> Option<Noticed<'a>> {
    let class = class(verdict)?;
    Some(Noticed { class, key: format!("{class}:{}@{}", name(path), ticket.head), noticed_by: LANDER, detail: verdict })
}

/// A candidate the lander held without a verdict, once per candidate and base, however many rounds it is held.
pub fn held<'a>(candidate: &str, base: &str, reason: &'a str) -> Option<Noticed<'a>> {
    let class = class(reason)?;
    Some(Noticed { class, key: format!("{class}:{candidate}@{base}"), noticed_by: LANDER, detail: reason })
}

pub fn arguments(path: &Path, ticket: &Ticket, noticed: &Noticed, at: f64) -> Vec<String> {
    let mut arguments: Vec<String> = ["incident", "record", "--key", &noticed.key, "--class", noticed.class, "--noticed-by", noticed.noticed_by].iter().map(|word| (*word).to_owned()).collect();
    let mut flag = |name: &str, value: String| {
        arguments.push(name.to_owned());
        arguments.push(value);
    };
    flag("--repo", telemetry::repository(&ticket.origin));
    flag("--subject", ticket.branch.clone());
    let queued = queue::queued_micros(path);
    if queued != u128::MAX {
        flag("--started-at", (queued as f64 / 1e6).to_string());
    }
    flag("--detected-at", at.to_string());
    flag("--detail", noticed.detail.to_owned());
    arguments
}

/// Records the incident through `north incident record` in the background, as the landings row is.
pub fn record(queue: &Path, path: &Path, ticket: &Ticket, noticed: Option<Noticed>) {
    let Some(noticed) = noticed else { return };
    if let Err(error) = records::spawn(queue, &arguments(path, ticket, &noticed, telemetry::epoch())) {
        println!("north land: could not record the {} incident for {}: {error}", noticed.class, ticket.branch);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ticket() -> Ticket {
        Ticket {
            origin: "https://github.com/tompassarelli/north.git".into(),
            root: PathBuf::from("/lanes/a"),
            host: "laptop".into(),
            branch: "a".into(),
            head: "f".repeat(40),
            session: None,
            queued_at: "2026-11-04T03:00:00Z".into(),
            first_commit_secs: None,
            several_commits: None,
            red_fix: None,
            verdict: None,
            keep_lane: false,
        }
    }

    #[test]
    fn every_verdict_but_landed_is_an_incident_class() {
        let classes: Vec<Option<&str>> = [
            "failed /logs/1-candidate.log",
            "conflict shared.txt",
            "transient fetch failed",
            "deferred DEFER moderate",
            "failed land check bun-test hung past its 600 s deadline",
            "transient land check x ran past its 600 s deadline and base tree abc has never been timed",
            "failed persistent transient: /logs/2.log",
            "withdrawn by proxy: superseded",
            "landed abc",
        ]
        .iter()
        .map(|verdict| class(verdict))
        .collect();
        assert_eq!(
            classes,
            [Some("lander-refused"), Some("lander-conflict"), Some("lander-transient"), Some("lander-deferred"), Some("lander-hang"), Some("lander-hang"), Some("lander-transient"), Some("lander-withdrawn"), None]
        );
    }

    #[test]
    fn a_decided_ticket_is_keyed_by_its_file_and_head_and_carries_its_queue_time() {
        let path = PathBuf::from("/queue/1793800600000000-a.json");
        let noticed = decided(&path, &ticket(), "conflict shared.txt").unwrap();
        assert_eq!(noticed.key, format!("lander-conflict:1793800600000000-a.json@{}", "f".repeat(40)));
        let text = arguments(&path, &ticket(), &noticed, 1_793_801_000.0).join(" ");
        for part in ["incident record", "--class lander-conflict --noticed-by lander", "--repo north", "--subject a", "--started-at 1793800600", "--detected-at 1793801000", "--detail conflict shared.txt"] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
        assert!(decided(&path, &ticket(), "landed abc").is_none());
        assert_eq!(held("c1", "b1", "transient fetch").unwrap().key, held("c1", "b1", "transient other words").unwrap().key);
        assert_ne!(held("c1", "b1", "transient fetch").unwrap().key, held("c1", "b2", "transient fetch").unwrap().key);
    }
}
