//! `north land doctor`: the lander's closed health loop (docs/features/04-landing-and-lanes/0028-lander-doctor.md).
//! Each run reads every lander health bound live, applies the known remedy for each known stuck state, records each
//! remedy as an incident row, keeps the lander builds that landed a batch green to roll back to, and pages the proxy
//! once per episode only for a state it has no remedy for. It may use north-base and north-landing, never the root crate.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use north_landing::land::lander::state_home;
use north_landing::land::{queue, repositories};

pub mod book;
pub mod drill;
pub mod bounds;
pub mod facts;
pub mod hands;

#[cfg(test)]
mod tests;

use book::Book;
use bounds::{Finding, PATIENCE_SECS, Remedy, State, minutes};
use facts::{Facts, Place, Reach};
use hands::Hands;

pub use north_landing::land::promote::REFUSED;
pub const RERAN: &str = "doctor/reran";
pub const BOOK: &str = "north/land-doctor/book.json";
pub const UNKNOWN: &str = "lander-unknown";
const USAGE: &str = "usage: north land doctor [--dry-run] (reads every lander health bound, applies each known remedy, pages the proxy once per unknown state)";

/// One finding and what the doctor did about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Treated {
    pub finding: Finding,
    pub outcomes: Vec<String>,
}

pub fn cure(facts: &Facts, book: &mut Book, hands: &dyn Hands, now: u64) -> Vec<Treated> {
    if let Some(link) = &facts.link {
        book.learn(link);
    }
    let findings = bounds::judge(facts, book, now);
    let over: BTreeSet<String> = findings.iter().filter(|finding| finding.over).map(Finding::key).collect();
    book.episodes.retain(|key, _| over.contains(key));
    book.checked_at = now;
    findings.into_iter().map(|finding| treat(finding, book, hands, now)).collect()
}

fn treat(finding: Finding, book: &mut Book, hands: &dyn Hands, now: u64) -> Treated {
    let key = finding.key();
    let mut outcomes = Vec::new();
    match &finding.state {
        State::Healthy => {}
        State::Known { remedies, .. } => {
            for remedy in remedies {
                let Some(class) = remedy.class() else {
                    outcomes.push(remedy.to_string());
                    continue;
                };
                match book.last_remedy(&key, class).map(|at| now.saturating_sub(at)) {
                    Some(waited) if waited < PATIENCE_SECS || matches!(remedy, Remedy::Alarm(_) | Remedy::Promote) => outcomes.push(format!("waiting: applied \"{remedy}\" {} ago", minutes(waited))),
                    Some(waited) => outcomes.push(page(&finding, &format!("\"{remedy}\" did not clear it {} after it was applied", minutes(waited)), book, hands, now)),
                    None => outcomes.push(apply(&finding, remedy, class, book, hands, now)),
                }
            }
        }
        State::Unknown(detail) => outcomes.push(page(&finding, detail, book, hands, now)),
    }
    Treated { finding, outcomes }
}

fn apply(finding: &Finding, remedy: &Remedy, class: &str, book: &mut Book, hands: &dyn Hands, now: u64) -> String {
    let key = finding.key();
    let result = hands.apply(remedy);
    let episode = book.open(&key, now);
    episode.remedies.push((now, class.to_owned()));
    let incident = format!("{class}:{key}@{}#{}", episode.since, episode.remedies.len());
    let outcome = match &result {
        Ok(said) => format!("applied \"{remedy}\": {said}"),
        Err(error) => format!("failed \"{remedy}\": {error}"),
    };
    if !hands.incident(class, &incident, &finding.subject, &format!("{} {} over {}: {outcome}", finding.bound.key(), finding.value, finding.bound.limit())) {
        return format!("{outcome}; the incident row was not recorded");
    }
    outcome
}

fn page(finding: &Finding, detail: &str, book: &mut Book, hands: &dyn Hands, now: u64) -> String {
    let key = finding.key();
    let episode = book.open(&key, now);
    if let Some(at) = episode.paged {
        return format!("unknown state: {detail}; the proxy heard of it {} ago", minutes(now.saturating_sub(at)));
    }
    let since = episode.since;
    let text = format!("north land doctor: unknown lander state: {} {} reads {} (bound {}): {detail}", finding.bound.key(), finding.subject, finding.value, finding.bound.limit());
    if !hands.page(&text) {
        return format!("unknown state: {detail}; paging the proxy failed, the next run tries again");
    }
    book.open(&key, now).paged = Some(now);
    hands.incident(UNKNOWN, &format!("{UNKNOWN}:{key}@{since}"), &finding.subject, detail);
    format!("unknown state: {detail}; paged the proxy")
}

pub fn render(treated: &[Treated]) -> String {
    let mut text = String::new();
    for entry in treated {
        let finding = &entry.finding;
        let verdict = match &finding.state {
            State::Healthy => "within".to_owned(),
            State::Known { name, .. } => format!("over: {name}"),
            State::Unknown(_) => "over: unknown state".to_owned(),
        };
        text.push_str(&format!("bound {} {}: {} (limit {}): {verdict}\n", finding.bound.key(), finding.subject, finding.value, finding.bound.limit()));
        for outcome in &entry.outcomes {
            text.push_str(&format!("  remedy: {outcome}\n"));
        }
    }
    text
}

/// Reads every bound on this lander host and its offload hosts.
pub fn examine(now: u64) -> Facts {
    let state = state_home();
    let host = repositories::local_host();
    let mut queues = Vec::new();
    let mut north = None;
    let discovery = repositories::discovered(&state, &host);
    for checkout in discovery.checkouts {
        let Ok(origin) = north_landing::land::candidate::git(&checkout, &["remote", "get-url", "--push", "origin"]) else { continue };
        let name = project(&checkout);
        if name == "north" {
            north = Some(checkout.clone());
        }
        queues.push(facts::queue_facts(&name, &queue::directory(&state, &origin), &checkout, now));
    }
    let units = queues.iter().map(|queue| queue.unit.clone()).collect();
    let link = facts::link_facts(&state.join("north"), north.as_deref(), units, now);
    let places = std::iter::once(Place { name: host, reach: Reach::Here }).chain(facts::offload_places()).map(facts::place_facts).collect();
    Facts { queues, link, places, unserved: discovery.unserved }
}

fn project(checkout: &Path) -> String {
    let named = if checkout.file_name().is_some_and(|name| name == "main") { checkout.parent() } else { Some(checkout) };
    named.and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

struct DryRun;

impl Hands for DryRun {
    fn apply(&self, remedy: &Remedy) -> Result<String, String> {
        Ok(format!("dry run: would {remedy}"))
    }

    fn incident(&self, _: &str, _: &str, _: &str, _: &str) -> bool {
        true
    }

    fn page(&self, _: &str) -> bool {
        false
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return 0;
        }
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let now = facts::now();
    let path: PathBuf = state_home().join(BOOK);
    let mut book = Book::load(&path);
    let facts = examine(now);
    let treated = if dry_run { cure(&facts, &mut book.clone(), &DryRun, now) } else { cure(&facts, &mut book, &hands::Host, now) };
    for line in render(&treated).lines() {
        println!("north land doctor: {line}");
    }
    if !dry_run && let Err(error) = book.save(&path) {
        eprintln!("north land doctor: {error}");
        return 1;
    }
    u8::from(treated.iter().any(|entry| matches!(entry.finding.state, State::Unknown(_))))
}
