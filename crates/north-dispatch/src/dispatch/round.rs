//! What one placing round came to, from what it already computed: the issues it found ready, the boxes it started and,
//! per limit, the ready issues it did not start, each counted once under the first limit that stopped it. Each real run
//! replaces `round.json` in the dispatch records, and `north work` prints it as one `dispatch round` line, read over ssh
//! when the dispatch host is another host.
use super::{IssueRef, Outcome, home, record};
use north_foundation::foundation::clock::Timestamp;
use north_pacing::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

const FILE: &str = "round.json";
const REACH: Duration = Duration::from_secs(10);
const OTHER_TEXT: usize = 90;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Round {
    pub at: f64,
    pub ready: usize,
    pub started: usize,
    pub not_started: BTreeMap<String, usize>,
    /// The first text of the most common reason no named limit covers, printed with the `other` count.
    #[serde(default)]
    pub other_largest: Option<String>,
}

/// The limit a refusal names, the first whose mark the refusal's text holds; None when no limit fits.
fn reason(text: &str) -> Option<&'static str> {
    const LIMITS: [(&str, &[&str]); 11] = [
        ("Claude session cap", &["at the dispatch_claude_sessions cap"]),
        ("DeepSeek slot cap", &["opencode workers run, the cap"]),
        ("no usage reading", &["has no usage reading"]),
        ("account refused", &["holds the box:", "has no account signed in", "is not signed in on"]),
        ("no host slot", &["has no free slot", "has no room for another build", "holds no box:"]),
        ("host takes nothing", &["takes nothing:"]),
        ("no host serves it", &["does not serve", "no host to place it on", "cannot read"]),
        ("host excluded", &["in dispatch_hosts_excluded"]),
        ("provider excluded", &["outside dispatch_providers"]),
        ("box held by a worker", &["` held:"]),
        ("box check retired", &["check retired"]),
    ];
    LIMITS.iter().find(|(_, marks)| marks.iter().any(|mark| text.contains(mark))).map(|(name, _)| *name)
}

/// The planned launches each issue kept running, after the ones that stopped.
fn boxes_started(outcome: &Outcome, stopped: &[(IssueRef, String)]) -> BTreeMap<IssueRef, usize> {
    let mut boxes: BTreeMap<IssueRef, usize> = BTreeMap::new();
    for launch in &outcome.launches {
        *boxes.entry(launch.item.clone()).or_default() += 1;
    }
    for (item, _) in stopped {
        if let Some(count) = boxes.get_mut(item) {
            *count = count.saturating_sub(1);
        }
    }
    boxes
}

/// Per issue, the limit that first named it, and else its first refusal no named limit covers; a launch that stopped names its issue too, after its refusals.
fn refusals<'a>(outcome: &'a Outcome, stopped: &[(IssueRef, String)]) -> (BTreeMap<IssueRef, &'static str>, BTreeMap<IssueRef, &'a str>) {
    let mut named: BTreeMap<IssueRef, &'static str> = BTreeMap::new();
    let mut unnamed: BTreeMap<IssueRef, &'a str> = BTreeMap::new();
    for (item, why) in &outcome.skipped {
        let text = why.strip_prefix("ready, not placed: ").or_else(|| why.strip_prefix("lead not placed: ")).unwrap_or(why);
        match reason(text) {
            Some(name) => {
                named.entry(item.clone()).or_insert(name);
            }
            None => {
                unnamed.entry(item.clone()).or_insert(text);
            }
        }
    }
    for (item, why) in stopped {
        named.entry(item.clone()).or_insert(if why.starts_with("skip ") { "skipped at launch" } else { "launch failed" });
    }
    (named, unnamed)
}

/// Counts each ready issue no box of which started, once, under its limit; the largest text an `other` count covers, when one does.
fn tally(ready: &[IssueRef], boxes: &BTreeMap<IssueRef, usize>, named: &BTreeMap<IssueRef, &'static str>, unnamed: &BTreeMap<IssueRef, &str>) -> (BTreeMap<String, usize>, Option<String>) {
    let mut not_started: BTreeMap<String, usize> = BTreeMap::new();
    let mut others: BTreeMap<&str, usize> = BTreeMap::new();
    for item in ready.iter().filter(|item| boxes.get(*item).is_none_or(|count| *count == 0)) {
        let name = match (named.get(item), unnamed.get(item)) {
            (Some(name), _) => *name,
            (None, Some(text)) => {
                *others.entry(*text).or_default() += 1;
                "other"
            }
            (None, None) => "no free box or worker cap",
        };
        *not_started.entry(name.to_owned()).or_default() += 1;
    }
    let largest = others.into_iter().max_by_key(|(_, count)| *count).map(|(text, _)| text.chars().take(OTHER_TEXT).collect());
    (not_started, largest)
}

impl Round {
    /// The round `outcome` planned, where `stopped` names the issue of each planned launch that did not start and why it did not.
    pub fn of(outcome: &Outcome, stopped: &[(IssueRef, String)], at: f64) -> Self {
        let boxes = boxes_started(outcome, stopped);
        let (named, unnamed) = refusals(outcome, stopped);
        let (not_started, other_largest) = tally(&outcome.ready, &boxes, &named, &unnamed);
        Self { at, ready: outcome.ready.len(), started: outcome.launches.len().saturating_sub(stopped.len()), not_started, other_largest }
    }

    pub fn line(&self) -> String {
        let held = if self.not_started.is_empty() {
            "none".to_owned()
        } else {
            self.not_started
                .iter()
                .map(|(why, count)| match (why.as_str(), &self.other_largest) {
                    ("other", Some(text)) => format!("{count} other, largest: {text}"),
                    _ => format!("{count} {why}"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!("dispatch round {}: {} ready issues, {} started; not started: {held}", Timestamp::from_unix_seconds_f64(self.at).local_twelve_hour(), self.ready, self.started)
    }

    pub fn save(&self) -> Result<(), String> {
        let directory = record::directory();
        let staged = directory.join(format!("{FILE}.tmp"));
        std::fs::create_dir_all(&directory).and_then(|()| std::fs::write(&staged, serde_json::to_vec(self).unwrap_or_default())).and_then(|()| std::fs::rename(&staged, directory.join(FILE))).map_err(|error| format!("{}: {error}", directory.join(FILE).display()))
    }
}

fn here() -> Result<String, String> {
    let path = record::directory().join(FILE);
    std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
}

/// `north fleet dispatch round`: prints this host's last round record.
pub fn serve() -> u8 {
    match here() {
        Ok(text) => {
            println!("{}", text.trim());
            0
        }
        Err(why) => {
            eprintln!("north fleet dispatch round: {why}");
            1
        }
    }
}

/// The dispatch host's last round, read here when this is the dispatch host, else over ssh.
pub fn last() -> Result<Round, String> {
    let cfg = placement::config()?;
    let text = match home::elsewhere(&cfg, &north_base::platform::host::name()) {
        None => here()?,
        Some((host, ssh)) => {
            let output = north_base::platform::process::run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &ssh, "north", "fleet", "dispatch", "round"], REACH).ok_or_else(|| format!("{host} did not answer within {}s", REACH.as_secs()))?;
            if !output.status.success() {
                return Err(format!("{host}: {}", String::from_utf8_lossy(&output.stderr).trim()));
            }
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
    };
    serde_json::from_str(text.trim()).map_err(|error| format!("round record unreadable: {error}"))
}
