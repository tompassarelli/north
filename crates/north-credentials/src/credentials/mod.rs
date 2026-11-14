mod accounts;
mod cookies;
pub mod declared;
mod model;
mod registry;
mod renew;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use model::{Credential, Kind, Renewal};

use north_base::platform::harness::{self, DEFAULTS};

const USAGE: &str = "Usage: north ops credentials check [--json] | north ops credentials renew [--dry-run] | north ops credentials import-cookies SITE [--from CHROME_PROFILE]";
const DAY: u64 = 86_400;
const NOTICE_KEY: &str = "credential_expiry_notice_days";
const ALERT_WITHIN: Duration = Duration::from_secs(30);

pub struct World {
    pub south: PathBuf,
    pub home: PathBuf,
    pub state: PathBuf,
    pub orchestration: PathBuf,
    pub chrome_profile: PathBuf,
    pub host: String,
    pub now: u64,
}

impl World {
    pub fn here() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = var("HOME").unwrap_or_default();
        let state = var("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        World {
            south: north_base::platform::shared_config::operator_root(&home),
            orchestration: north_base::platform::orchestration::path(),
            chrome_profile: var("AUTOMATION_CHROME_PROFILE").unwrap_or_else(|| state.join("automation-chrome/profile")),
            host: north_base::platform::host::name(),
            now: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs()),
            state,
            home,
        }
    }
}

pub fn notice_days() -> u64 {
    harness::document(DEFAULTS)[NOTICE_KEY]["value"].as_integer().and_then(|days| u64::try_from(days).ok()).unwrap_or_else(|| panic!("harness/defaults.toml has no whole number for {NOTICE_KEY}"))
}

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let world = World::here();
    match words[..] {
        ["check"] => check(&world, false),
        ["check", "--json"] => check(&world, true),
        ["renew"] => renew(&world, false),
        ["renew", "--dry-run"] => renew(&world, true),
        ["import-cookies", site] => import_cookies(&world, site, &world.home.join(".config/google-chrome")),
        ["import-cookies", site, "--from", profile] => import_cookies(&world, site, Path::new(profile)),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn gathered(world: &World) -> Option<Vec<Credential>> {
    registry::gather(world).map_err(|error| eprintln!("north ops credentials: {error}")).ok()
}

fn check(world: &World, json: bool) -> u8 {
    let Some(credentials) = gathered(world) else { return 1 };
    let failing = registry::failing(&credentials, world.now);
    if json {
        println!("{}", registry::json(&credentials, world.now));
    } else {
        registry::lines(&credentials, world.now).iter().for_each(|line| println!("{line}"));
        println!("{} credentials, {} failing", credentials.len(), failing.len());
    }
    u8::from(!failing.is_empty())
}

struct Machine {
    dry_run: bool,
}

impl renew::Effects for Machine {
    fn run(&self, command: &[String]) -> Result<(), String> {
        let Some((program, rest)) = command.split_first() else { return Err("empty renewal command".into()) };
        let status = Command::new(program).args(rest).status().map_err(|error| format!("{program}: {error}"))?;
        status.success().then_some(()).ok_or_else(|| format!("{program} exited {status}"))
    }

    fn alert(&self, text: &str) -> Result<(), String> {
        let args = ["fleet", "send", "--from", "service:north-credentials", "proxy", text];
        match north_base::platform::process::run_within("north", &args, ALERT_WITHIN) {
            Some(output) if output.status.success() => Ok(()),
            _ => Err("north fleet send to the proxy failed".into()),
        }
    }

    fn dry_run(&self) -> bool {
        self.dry_run
    }
}

fn renew(world: &World, dry_run: bool) -> u8 {
    let Some(credentials) = gathered(world) else { return 1 };
    let record = world.state.join(renew::RECORD);
    let outcome = renew::tick(&credentials, &world.host, world.now, notice_days(), &record, &Machine { dry_run });
    outcome.lines.iter().for_each(|line| println!("{line}"));
    u8::from(outcome.failed)
}

fn import_cookies(world: &World, site: &str, from: &Path) -> u8 {
    let outcome = declared::read(&world.south).and_then(|declarations| cookies::import(&cookies::Import { site, from, to: &world.chrome_profile }, &declarations, keyring_password));
    match outcome {
        Ok(count) => {
            println!("imported {count} cookies for {site} into {}", world.chrome_profile.display());
            0
        }
        Err(error) => {
            eprintln!("north ops credentials: {error}");
            1
        }
    }
}

fn keyring_password() -> Result<Vec<u8>, String> {
    let output = Command::new("secret-tool").args(["lookup", "application", "chrome"]).output().map_err(|error| format!("secret-tool: {error}"))?;
    if output.status.success() && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err("the Chrome Safe Storage key is not in the unlocked Secret Service".into())
    }
}
