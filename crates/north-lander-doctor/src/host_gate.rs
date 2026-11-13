use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use north_base::platform::harness::{DEFAULTS, harness_root};
use north_base::platform::land_queue::rfc3339_seconds;
use north_lander::land::lander::state_home;
use north_lander::land::{candidate, queue, repositories};

use crate::facts;

const SWITCH: &str = "laptop_system_check_gate";
const SKIP: u8 = 3;
const WAIT: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_secs(15);
const STALE_SECS: u64 = 45 * 60;
const USAGE: &str = "usage: north land host-gate --switch | --repo DIR --host NAME --fact FILE (exit 0: build NAME's toplevel; 3: skip it, since the toplevel is unchanged since the fact's build or every wait found a landing queue holding tickets)";

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [flag] if flag == "--switch") {
        println!("{}", if switched_on() { "on" } else { "off" });
        return 0;
    }
    let Some(asked) = Asked::parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if !switched_on() {
        say(&format!("the switch {SWITCH} is off, so {}'s toplevel is built as before", asked.host));
        return 0;
    }
    asked.decide()
}

struct Asked {
    repo: PathBuf,
    host: String,
    fact: PathBuf,
}

impl Asked {
    fn parse(arguments: &[String]) -> Option<Self> {
        let (mut repo, mut host, mut fact) = (None, None, None);
        let mut words = arguments.iter();
        while let Some(flag) = words.next() {
            let value = words.next()?;
            match flag.as_str() {
                "--repo" => repo = Some(PathBuf::from(value)),
                "--host" => host = Some(value.clone()),
                "--fact" => fact = Some(PathBuf::from(value)),
                _ => return None,
            }
        }
        Some(Self { repo: repo?, host: host?, fact: fact? })
    }

    fn decide(&self) -> u8 {
        let fact = fs::read_to_string(&self.fact).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let age = fact.as_ref().and_then(|fact| fact["observed_at"].as_str()).and_then(rfc3339_seconds).map(|at| facts::now().saturating_sub(at as u64));
        match age.filter(|age| *age <= STALE_SECS) {
            Some(age) => {
                let busy = quiet_within(WAIT);
                if !busy.is_empty() {
                    say(&format!("deferred: the landing queue of {} still holds tickets after {} s and the last fact is {age} s old, so the fact stays as it is", busy.join(", "), WAIT.as_secs()));
                    return SKIP;
                }
            }
            None => say(&format!("the last fact is missing or older than {} min, so the check goes on without waiting for the landing queues", STALE_SECS / 60)),
        }
        let Ok(head) = candidate::git(&self.repo, &["rev-parse", "HEAD"]) else { return 0 };
        let Some((fact, base)) = fact.and_then(|fact| self.built_base(&fact, &head).map(|base| (fact, base))) else {
            say(&format!("no buildable fact names an ancestor of {head}, so {}'s toplevel is built", self.host));
            return 0;
        };
        if base != head && !self.unchanged(&base, &head) {
            return 0;
        }
        match refresh(&self.fact, fact, &head, &base) {
            Ok(()) => {
                say(&format!("{}'s toplevel at {head} is the one built at {base}, so the fact moves to {head} without a build", self.host));
                SKIP
            }
            Err(error) => {
                say(&format!("could not refresh {}: {error}, so the toplevel is built", self.fact.display()));
                0
            }
        }
    }

    fn built_base(&self, fact: &Value, head: &str) -> Option<String> {
        let base = fact["inputs"]["nixos_config"].as_str().filter(|_| fact["buildable"].as_bool() == Some(true))?.to_owned();
        candidate::git(&self.repo, &["merge-base", "--is-ancestor", &base, head]).ok().map(|_| base)
    }

    fn unchanged(&self, base: &str, head: &str) -> bool {
        let output = Command::new("nice")
            .args(["-n", "19", "bash", ".github/scripts/host-diff.sh", "select"])
            .env("SAFE_PUSH_LAND_BASE", base)
            .env("SAFE_PUSH_LAND_HEAD", head)
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output();
        let Ok(output) = output.inspect_err(|error| say(&format!("host-diff.sh did not start: {error}"))) else { return false };
        let text = String::from_utf8_lossy(&output.stdout);
        print!("{text}");
        let reused = format!("host check: reused nixosConfigurations.{} ", self.host);
        output.status.success() && text.lines().any(|line| line.starts_with(&reused) || line.starts_with("host check: skipped"))
    }
}

fn quiet_within(wait: Duration) -> Vec<String> {
    let started = Instant::now();
    loop {
        let busy = busy(&queues());
        if busy.is_empty() || started.elapsed() >= wait {
            return busy;
        }
        sleep(POLL);
    }
}

fn queues() -> Vec<(String, PathBuf)> {
    let state = state_home();
    let discovery = repositories::discovered(&state, &repositories::local_host());
    discovery
        .checkouts
        .iter()
        .filter_map(|checkout| {
            let origin = candidate::git(checkout, &["remote", "get-url", "--push", "origin"]).ok()?;
            Some((crate::project(checkout), queue::directory(&state, &origin)))
        })
        .collect()
}

pub fn busy(queues: &[(String, PathBuf)]) -> Vec<String> {
    queues.iter().filter(|(_, directory)| queue::pending(directory).is_ok_and(|pending| !pending.is_empty())).map(|(name, _)| name.clone()).collect()
}

fn switched_on() -> bool {
    let Ok(root) = harness_root() else { return false };
    let Ok(text) = fs::read_to_string(root.join(DEFAULTS)) else { return false };
    text.parse::<toml_edit::DocumentMut>().ok().and_then(|document| document.get(SWITCH)?.get("value")?.as_str().map(|value| value == "on")).unwrap_or(false)
}

fn refresh(path: &Path, mut fact: Value, head: &str, base: &str) -> Result<(), String> {
    fact["observed_at"] = Value::from(stamp(facts::now()));
    fact["inputs"]["nixos_config"] = Value::from(head);
    fact["reused_from"] = Value::from(base);
    let text = format!("{}\n", serde_json::to_string(&fact).map_err(|error| error.to_string())?);
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let digest = format!("{:x}  {name}\n", Sha256::digest(text.as_bytes()));
    let beside = |suffix: &str| path.with_file_name(format!("{name}{suffix}.{}", std::process::id()));
    let (fact_tmp, digest_tmp) = (beside(".tmp"), beside(".sha256.tmp"));
    fs::write(&fact_tmp, text).and_then(|()| fs::write(&digest_tmp, digest)).map_err(|error| error.to_string())?;
    fs::rename(&digest_tmp, path.with_file_name(format!("{name}.sha256"))).and_then(|()| fs::rename(&fact_tmp, path)).map_err(|error| error.to_string())
}

fn stamp(seconds: u64) -> String {
    let at = time::OffsetDateTime::from_unix_timestamp(i64::try_from(seconds).unwrap_or_default()).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second())
}

fn say(text: &str) {
    println!("north land host-gate: {text}");
}
