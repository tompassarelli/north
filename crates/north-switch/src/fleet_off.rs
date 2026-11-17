//! `north fleet off`: on every fleet host at once, freeze landing and stop the units that start work (dispatch,
//! the tick and every other fleet timer), give working sessions until the grace runs out to finish their step,
//! then stop the fleet's sessions and units and kill whatever still runs at the deadline. `--force` kills them at
//! once. It pauses dispatch where no pause stands, so a deploy that re-enables a timer still launches nothing. It disables each enabled unit it stops, so a restart of the user manager or the host leaves it stopped. It records what it stopped for `north fleet on` and parks the stopped sessions in the cached fleet table.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::config::Config;
use crate::host::{self, Host, each};
use crate::paths::Paths;
use crate::record::{self, Entry};
use crate::table;
use crate::units;

const GRACE: Duration = Duration::from_secs(600);
const STOP_WINDOW: Duration = Duration::from_secs(90);
const FORCE_WINDOW: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_secs(2);
const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
const WORKING: [&str; 2] = ["working", "starting"];
const SESSIONS: &str = "n=\"$HOME/.local/state/north/current/north\"; if [ -x \"$n\" ]; then \"$n\" fleet list --json 2>/dev/null || echo '[]'; else echo '[]'; fi";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Graceful,
    Force,
}

#[derive(Default)]
struct Reach {
    hostname: String,
    froze: bool,
    stopped: Vec<Entry>,
    left: Vec<String>,
    problems: Vec<String>,
}

struct Fleet<'a> {
    config: &'a Config,
    own: BTreeSet<String>,
    caller_host: String,
}

pub fn off(mode: Mode) -> ExitCode {
    match run(mode) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("north fleet off: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(mode: Mode) -> Result<ExitCode, String> {
    let paths = Paths::here()?;
    let config = Config::load(&paths.config_home)?;
    let hosts = Host::fleet(&config.hosts)?;
    let caller = env::var(SESSION_ENV).ok().filter(|name| !name.is_empty());
    let fleet = Fleet { config: &config, own: own_units(), caller_host: host::here() };
    let started = Instant::now();
    let quieted = each(&hosts, |host| fleet.quiet(host));
    if mode == Mode::Graceful {
        wait_for_steps(&hosts, caller.as_deref(), started + GRACE - STOP_WINDOW);
    }
    let deadline = match mode {
        Mode::Graceful => started + GRACE,
        Mode::Force => Instant::now() + FORCE_WINDOW,
    };
    let halted = each(&hosts, |host| fleet.halt(host, mode, deadline));
    let mut entries = record::read(&paths.state_home)?;
    let mut stopped_hosts = BTreeSet::new();
    for ((host, mut reach), halt) in hosts.iter().zip(quieted).zip(halted) {
        reach.absorb(halt);
        report(&host.name, &reach);
        if reach.clean() {
            stopped_hosts.insert(reach.hostname.clone());
        }
        entries.extend(reach.stopped);
    }
    let clean = stopped_hosts.len() == hosts.len();
    record::write(&paths.state_home, &entries)?;
    match table::park(&paths.state_home, &stopped_hosts, caller.as_deref()) {
        Ok(parked) if !parked.is_empty() => println!("north fleet off: parked {} sessions in this host's fleet table: {}", parked.len(), parked.join(", ")),
        Ok(_) => {}
        Err(error) => println!("north fleet off: the cached fleet table was left as it was: {error}"),
    }
    let units = entries.iter().filter(|entry| matches!(entry, Entry::Unit { .. })).count();
    if clean {
        println!("north fleet off: nothing of the fleet runs on {} hosts; `north fleet on` enables and starts the {units} units it stopped", hosts.len());
        return Ok(ExitCode::SUCCESS);
    }
    println!("north fleet off: not everything stopped (above); `north fleet off --force` kills what is left, and `north fleet on` enables and starts the {units} units it stopped");
    Ok(ExitCode::from(1))
}

impl Reach {
    fn absorb(&mut self, later: Self) {
        self.stopped.extend(later.stopped);
        self.left = later.left;
        for problem in later.problems {
            if !self.problems.contains(&problem) {
                self.problems.push(problem);
            }
        }
        if self.hostname.is_empty() {
            self.hostname = later.hostname;
        }
    }

    fn clean(&self) -> bool {
        self.left.is_empty() && self.problems.is_empty() && !self.hostname.is_empty()
    }
}

impl Fleet<'_> {
    fn quiet(&self, host: &Host) -> Reach {
        let mut reach = Reach::default();
        let listing = match units::list(host) {
            Ok(listing) => listing,
            Err(error) => {
                reach.problems.push(format!("unreachable: {error}"));
                return reach;
            }
        };
        reach.hostname = listing.hostname;
        match host.run(&freeze_script(&self.caller_host)) {
            Ok(said) if said.trim() == "placed" => {
                reach.froze = true;
                reach.stopped.push(Entry::Freeze { host: host.name.clone() });
            }
            Ok(_) => {}
            Err(error) => reach.problems.push(format!("landing not frozen: {error}")),
        }
        match host.run(&pause_script(&self.caller_host)) {
            Ok(said) if said.trim() == "placed" => reach.stopped.push(Entry::Pause { host: host.name.clone() }),
            Ok(_) => {}
            Err(error) => reach.problems.push(format!("dispatch not paused: {error}")),
        }
        let triggers: Vec<String> = listing.active.into_iter().filter(|unit| units::is_trigger(unit) && self.ours(host, unit)).collect();
        self.stop(host, &triggers, &mut reach);
        reach
    }

    fn halt(&self, host: &Host, mode: Mode, deadline: Instant) -> Reach {
        let mut reach = Reach::default();
        let listing = match units::list(host) {
            Ok(listing) => listing,
            Err(error) => {
                reach.problems.push(format!("unreachable: {error}"));
                return reach;
            }
        };
        reach.hostname = listing.hostname;
        let work: Vec<String> = listing.active.into_iter().filter(|unit| !units::is_trigger(unit) && self.ours(host, unit)).collect();
        if mode == Mode::Force
            && let Err(error) = units::kill(host, &work)
        {
            reach.problems.push(format!("kill: {error}"));
        }
        self.stop(host, &work, &mut reach);
        reach.left = self.wait_gone(host, &work, deadline);
        reach
    }

    fn stop(&self, host: &Host, names: &[String], reach: &mut Reach) {
        let described = match units::described(host, names) {
            Ok(described) => described,
            Err(error) => {
                reach.problems.push(format!("cannot read units: {error}"));
                return;
            }
        };
        let enabled: Vec<String> = described.iter().filter(|unit| unit.enabled).map(|unit| unit.name.clone()).collect();
        if let Err(error) = units::disable(host, &enabled) {
            reach.problems.push(format!("disable: {error}"));
        }
        let stopped = if names.iter().all(|name| units::is_trigger(name)) { units::stop(host, names) } else { units::stop_now(host, names) };
        if let Err(error) = stopped {
            reach.problems.push(format!("stop: {error}"));
        }
        reach.stopped.extend(described.iter().map(|unit| Entry::unit(&host.name, unit)));
    }

    fn wait_gone(&self, host: &Host, names: &[String], deadline: Instant) -> Vec<String> {
        loop {
            let running = self.running(host);
            let remaining: Vec<String> = names.iter().filter(|name| running.contains(*name)).cloned().collect();
            if remaining.is_empty() || Instant::now() >= deadline {
                if !remaining.is_empty() {
                    let _ = units::kill(host, &remaining);
                    let _ = units::stop_now(host, &remaining);
                    thread::sleep(POLL);
                }
                return self.running(host);
            }
            thread::sleep(POLL);
        }
    }

    fn running(&self, host: &Host) -> Vec<String> {
        units::list(host).map(|listing| listing.active.into_iter().filter(|unit| self.ours(host, unit)).collect()).unwrap_or_default()
    }

    fn ours(&self, host: &Host, unit: &str) -> bool {
        self.config.fleet_unit(unit) && !(host.is_here() && self.own.contains(unit))
    }
}

fn wait_for_steps(hosts: &[Host], caller: Option<&str>, until: Instant) {
    let mut told = false;
    loop {
        let working: BTreeSet<String> = each(hosts, working_sessions).into_iter().flatten().filter(|name| Some(name.as_str()) != caller).collect();
        if working.is_empty() {
            return;
        }
        let names = working.iter().cloned().collect::<Vec<_>>().join(", ");
        if Instant::now() >= until {
            println!("north fleet off: still working at the end of the grace, so stopped mid-step: {names}");
            return;
        }
        if !told {
            println!("north fleet off: waiting up to {} s for {} working sessions to finish their step: {names}", until.saturating_duration_since(Instant::now()).as_secs(), working.len());
            told = true;
        }
        thread::sleep(POLL);
    }
}

fn working_sessions(host: &Host) -> Vec<String> {
    let rows: Value = host.run(SESSIONS).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    rows.as_array()
        .into_iter()
        .flatten()
        .filter(|row| row.get("status").and_then(Value::as_str).is_some_and(|status| WORKING.contains(&status)))
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn freeze_script(caller_host: &str) -> String {
    let placed_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let freeze = serde_json::json!({ "cause": format!("north fleet off from {caller_host}"), "placed_at": placed_at, "placed_on": caller_host });
    format!(
        "d=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north\"; f=\"$d/land-freeze.json\"; if [ -e \"$f\" ]; then echo held; else mkdir -p \"$d\" && printf '%s\\n' {} > \"$f.new\" && mv \"$f.new\" \"$f\" && echo placed; fi",
        host::quote(&freeze.to_string())
    )
}

fn pause_script(caller_host: &str) -> String {
    let pause = serde_json::json!({ "reason": format!("north fleet off from {caller_host}"), "set_by": "north fleet off", "since": "%s", "check": "false" });
    format!(
        "d=\"$HOME/.local/state/north\"; f=\"$d/dispatch-paused\"; if [ -e \"$f\" ]; then echo held; else mkdir -p \"$d\" && printf {} \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\" > \"$f.new\" && mv \"$f.new\" \"$f\" && echo placed; fi",
        host::quote(&format!("{pause}\n"))
    )
}

fn report(name: &str, reach: &Reach) {
    let host = if reach.hostname.is_empty() || reach.hostname == name { name.to_owned() } else { format!("{name} ({})", reach.hostname) };
    if reach.froze {
        println!("north fleet off: {host}: landing frozen");
    }
    if reach.stopped.iter().any(|entry| matches!(entry, Entry::Pause { .. })) {
        println!("north fleet off: {host}: dispatch paused");
    }
    let stopped: Vec<&str> = reach.stopped.iter().filter_map(|entry| if let Entry::Unit { name, .. } = entry { Some(name.as_str()) } else { None }).collect();
    let disabled = reach.stopped.iter().filter(|entry| matches!(entry, Entry::Unit { disabled: true, .. })).count();
    match stopped.is_empty() {
        true if reach.hostname.is_empty() => {}
        true => println!("north fleet off: {host}: no fleet unit was running"),
        false => println!("north fleet off: {host}: stopped {} ({disabled} also disabled until `north fleet on`): {}", stopped.len(), stopped.join(" ")),
    }
    if !reach.left.is_empty() {
        println!("north fleet off: {host}: still running: {}", reach.left.join(" "));
    }
    for problem in &reach.problems {
        println!("north fleet off: {host}: {problem}");
    }
}

fn own_units() -> BTreeSet<String> {
    fs::read_to_string("/proc/self/cgroup")
        .unwrap_or_default()
        .lines()
        .flat_map(|line| line.rsplit_once(':').map_or("", |(_, path)| path).split('/'))
        .filter(|part| part.ends_with(".service") || part.ends_with(".scope"))
        .map(str::to_owned)
        .collect()
}
