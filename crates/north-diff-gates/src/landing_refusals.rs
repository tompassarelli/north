//! Which landing refusals stand (north#729). A broken build, clippy, failing tests, a secret, a privacy leak or a test the
//! repository's test policy does not admit always refuses a lane; every other landing gate follows `landing_refusals`
//! in harness/defaults.toml. In shadow the gate's would-be refusal is appended to [`LOG`] under the state home and the lane lands; `north land gate-stats` prints that
//! log beside the real refusals, so a gate earns refusal back with a would-be refusal that names a real fault.
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::harness::{DEFAULTS, document};

pub const LOG: &str = "north/gates/landing-shadow.jsonl";
const DETAIL_CHARS: usize = 4000;

/// Whether every landing gate refuses, read from `landing_refusals`; any value but `enforce` keeps them in shadow.
pub fn enforcing() -> bool {
    document(DEFAULTS)["landing_refusals"]["value"].as_str() == Some("enforce")
}

pub const TEST_ADMISSION: &str = "test admission";
pub const REBASE_COMPILE: &str = "rebase compile";

/// Whether `gates`, one gate's label or several joined by `, `, names a gate that refuses whatever `landing_refusals` says: test admission, whose refusals are the repository's test policy (north#1048), and the compile of a lane rebased onto a moved main, which is a broken build (north#1036).
pub fn always_refuses(gates: &str) -> bool {
    gates.split(", ").any(|gate| gate == TEST_ADMISSION || gate == REBASE_COMPILE)
}

/// The lane a gate judged and the place it judged it: `enqueue`, `pick` or `candidate`.
pub struct Lane<'a> {
    pub at: &'a str,
    pub root: &'a str,
    pub branch: &'a str,
    pub head: &'a str,
}

impl Lane<'_> {
    /// `outcome` as it stands: a refusal under enforce, else its would-be refusal logged under `state_home` and passed.
    pub fn judged(&self, state_home: &Path, gate: &str, outcome: Result<(), String>) -> Result<(), String> {
        match outcome {
            Err(detail) if !enforcing() && !always_refuses(gate) => {
                self.record(state_home, gate, &detail);
                Ok(())
            }
            other => other,
        }
    }

    /// Appends `gate`'s would-be refusal to the shadow log and says so on stderr.
    pub fn record(&self, state_home: &Path, gate: &str, detail: &str) {
        eprintln!("safe-push: ⚠ {gate} would refuse {} here; landing it anyway while landing_refusals in harness/defaults.toml is shadow:\n{}", self.head, detail.trim_end());
        let row = serde_json::json!({
            "micros": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)),
            "gate": gate,
            "at": self.at,
            "root": self.root,
            "branch": self.branch,
            "head": self.head,
            "detail": detail.chars().take(DETAIL_CHARS).collect::<String>(),
        });
        let path = state_home.join(LOG);
        let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| OpenOptions::new().create(true).append(true).open(&path)).and_then(|mut file| writeln!(file, "{row}"));
        if let Err(why) = written {
            eprintln!("safe-push: the shadow row for {gate} was not written to {}: {why}", path.display());
        }
    }
}
