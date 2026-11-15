//! What one placing round came to, from what it already computed: the issues it found ready, the boxes it started and,
//! per reason, the ready boxes it did not start. Each real run replaces `round.json` in the dispatch records, and
//! `north work` prints it as one `dispatch round` line, read over ssh when the dispatch host is another host.
use super::{Outcome, home, record};
use north_foundation::foundation::clock::Timestamp;
use north_pacing::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

const FILE: &str = "round.json";
const REACH: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Round {
    pub at: f64,
    pub ready: usize,
    pub started: usize,
    pub not_started: BTreeMap<String, usize>,
}

/// The limit a placement refusal names, the first that holds across all the hosts it lists.
fn reason(why: &str) -> &'static str {
    const LIMITS: [(&str, &[&str]); 6] = [
        ("Claude session cap", &["at the dispatch_claude_sessions cap"]),
        ("DeepSeek slot cap", &["opencode workers run, the cap"]),
        ("no usage reading", &["has no usage reading"]),
        ("account refused", &["holds the box:", "has no account signed in"]),
        ("no host slot", &["has no free slot", "has no room for another build", "holds no box:", "takes nothing:"]),
        ("no host serves it", &["does not serve", "no host to place it on", "cannot read"]),
    ];
    LIMITS.iter().find(|(_, marks)| marks.iter().any(|mark| why.contains(mark))).map_or("other", |(name, _)| *name)
}

impl Round {
    /// The round `outcome` planned, where `stopped` holds why each planned launch that did not start did not.
    pub fn of(outcome: &Outcome, stopped: &[String], at: f64) -> Self {
        let mut not_started: BTreeMap<String, usize> = BTreeMap::new();
        let refused = outcome.skipped.iter().filter_map(|(_, why)| why.strip_prefix("ready, not placed: ").or_else(|| why.strip_prefix("lead not placed: ")));
        for why in refused {
            *not_started.entry(reason(why).to_owned()).or_default() += 1;
        }
        for why in stopped {
            *not_started.entry(if why.starts_with("skip ") { "skipped at launch" } else { "launch failed" }.to_owned()).or_default() += 1;
        }
        Self { at, ready: outcome.ready.len(), started: outcome.launches.len().saturating_sub(stopped.len()), not_started }
    }

    pub fn line(&self) -> String {
        let held = if self.not_started.is_empty() { "none".to_owned() } else { self.not_started.iter().map(|(why, count)| format!("{count} {why}")).collect::<Vec<_>>().join(", ") };
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
