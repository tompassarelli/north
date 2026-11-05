use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use toml_edit::DocumentMut;

use super::agent_load::{self, Host};
use super::game_reserve;

const USAGE: &str = "usage: north capacity mode status | tick | hold [--minutes N] [PHRASE] | extend | release";

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

pub fn number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub fn strings(name: &str) -> Vec<String> {
    let list = DEFAULTS[name]["value"].as_array().unwrap_or_else(|| panic!("harness/defaults.toml has no list for {name}"));
    list.iter().map(|item| item.as_str().unwrap_or_else(|| panic!("harness/defaults.toml {name} holds a non-string")).to_owned()).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Away,
    #[default]
    Desktop,
    Play,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Away => "away",
            Self::Desktop => "desktop",
            Self::Play => "play",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub reserve: bool,
    pub hold: bool,
    pub pause_heavy: bool,
}

pub fn effects(table: &[String], mode: Mode) -> Effects {
    let flags = table.iter().find_map(|row| row.strip_prefix(mode.name())?.strip_prefix(':')).unwrap_or_default();
    let has = |flag: &str| flags.split(',').map(str::trim).any(|item| item == flag);
    Effects { reserve: has("reserve"), hold: has("hold"), pause_heavy: has("pause-heavy") }
}

impl Effects {
    pub fn confined(self, enforced: bool) -> Self {
        Self { pause_heavy: self.pause_heavy || (self.reserve && !enforced), ..self }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hold {
    pub since: u64,
    pub until: u64,
    pub phrase: String,
    #[serde(default)]
    pub minutes: u64,
    #[serde(default)]
    pub warned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub hold: Option<Hold>,
    #[serde(default)]
    pub play_cpus: Option<u32>,
    #[serde(default)]
    pub calm_since: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub cap_seconds: u64,
    pub lease_minutes: u64,
    pub warn_seconds: u64,
    pub play_cpus: u32,
    pub play_cpus_min: u32,
    pub play_cpus_max: u32,
    pub play_cpus_step: u32,
    pub psi_target: f64,
    pub calm_seconds: u64,
    pub table: Vec<String>,
}

impl Limits {
    pub fn bundled() -> Self {
        let whole = |name: &str| number(name) as u64;
        let cpus = |name: &str| number(name) as u32;
        Self {
            cap_seconds: whole("capacity_mode_hold_sec"),
            lease_minutes: whole("capacity_mode_lease_default_min"),
            warn_seconds: whole("capacity_mode_lease_warn_sec"),
            play_cpus: cpus("capacity_mode_play_cpus"),
            play_cpus_min: cpus("capacity_mode_play_cpus_min"),
            play_cpus_max: cpus("capacity_mode_play_cpus_max"),
            play_cpus_step: cpus("capacity_mode_play_cpus_step"),
            psi_target: number("capacity_mode_play_psi_target"),
            calm_seconds: whole("capacity_mode_play_calm_sec"),
            table: strings("capacity_mode_table"),
        }
    }

    pub fn effects(&self, mode: Mode) -> Effects {
        effects(&self.table, mode)
    }
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub now: u64,
    pub game_running: bool,
    pub seat_idle: Option<bool>,
    pub protected_psi: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Adjustment {
    pub at: u64,
    pub from: u32,
    pub to: u32,
    pub protected_cpu_some_avg10: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stepped {
    pub adjustment: Option<Adjustment>,
    pub notice: Option<String>,
}

pub fn step(state: &State, seen: &Observation, limits: &Limits) -> (State, Stepped) {
    let mut next = state.clone();
    let mut notice = None;
    if let Some(hold) = next.hold.as_mut() {
        if seen.now >= hold.until {
            notice = Some(format!("Playtest lease returned at {}; the laptop is working again.", clock(hold.until)));
            next.hold = None;
        } else if !hold.warned && hold.until.saturating_sub(seen.now) <= limits.warn_seconds {
            hold.warned = true;
            notice = Some(format!("Playtest lease ends at {}. Reply more to add {} min.", clock(hold.until), hold.minutes));
        }
    }
    let (mode, reason) = classify(&next, seen);
    if mode != state.mode || state.since == 0 {
        next.since = seen.now;
        next.calm_since = None;
    }
    next.mode = mode;
    next.reason = reason;
    let adjustment = if mode == Mode::Play { size(&mut next, seen, limits) } else { None };
    (next, Stepped { adjustment, notice })
}

fn classify(state: &State, seen: &Observation) -> (Mode, String) {
    if let Some(hold) = &state.hold {
        return (Mode::Play, format!("playtest lease from Tom's prompt \"{}\" until {}", hold.phrase, clock(hold.until)));
    }
    let game = if seen.game_running { "; Tom's game is open" } else { "" };
    match seen.seat_idle {
        Some(true) => (Mode::Away, format!("the seat is idle and no playtest lease is on{game}")),
        Some(false) => (Mode::Desktop, format!("the seat is in use and no playtest lease is on{game}")),
        None => (Mode::Desktop, format!("the seat idle hint is unreadable and no playtest lease is on{game}")),
    }
}

fn size(next: &mut State, seen: &Observation, limits: &Limits) -> Option<Adjustment> {
    let current = next.play_cpus.unwrap_or(limits.play_cpus).clamp(limits.play_cpus_min, limits.play_cpus_max);
    next.play_cpus = Some(current);
    let psi = seen.protected_psi?;
    let to = if psi > limits.psi_target {
        next.calm_since = None;
        (current + limits.play_cpus_step).min(limits.play_cpus_max)
    } else {
        let calm = *next.calm_since.get_or_insert(seen.now);
        if seen.now.saturating_sub(calm) >= limits.calm_seconds {
            next.calm_since = Some(seen.now);
            current.saturating_sub(limits.play_cpus_step).max(limits.play_cpus_min)
        } else {
            current
        }
    };
    next.play_cpus = Some(to);
    (to != current).then_some(Adjustment { at: seen.now, from: current, to, protected_cpu_some_avg10: psi })
}

pub fn hold(state: &State, now: u64, phrase: &str, minutes: u64, limits: &Limits) -> State {
    let mut next = state.clone();
    let active = next.hold.as_ref().filter(|hold| now < hold.until);
    let until = (active.map_or(now, |hold| hold.until) + minutes * 60).min(now + limits.cap_seconds);
    let since = active.map_or(now, |hold| hold.since);
    next.hold = Some(Hold { since, until, phrase: phrase.to_owned(), minutes, warned: false });
    if next.mode != Mode::Play {
        next.since = now;
    }
    next.mode = Mode::Play;
    next.reason = format!("playtest lease from Tom's prompt \"{phrase}\" until {}", clock(until));
    next
}

pub fn extend(state: &State, now: u64, limits: &Limits) -> Option<State> {
    let hold = state.hold.as_ref().filter(|hold| now < hold.until)?;
    Some(self::hold(state, now, &hold.phrase, hold.minutes.max(1), limits))
}

pub fn release(state: &State, now: u64) -> State {
    let mut next = state.clone();
    next.hold = None;
    next.calm_since = None;
    next.mode = Mode::Desktop;
    next.since = now;
    next.reason = "released by Tom's prompt".to_owned();
    next
}

pub fn cpu_split(total: u32, game: u32) -> (String, String) {
    let half = (total / 2).max(1);
    let cores = (game / 2).clamp(1, half.saturating_sub(1).max(1));
    let game = format!("0-{},{}-{}", cores - 1, half, half + cores - 1);
    let agents = if cores >= half { String::new() } else { format!("{}-{},{}-{}", cores, half - 1, half + cores, total - 1) };
    (game, agents)
}

fn clock(at: u64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(at).unwrap_or(i64::MAX)).local_twelve_hour()
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("CAPACITY_MODE_STATE").filter(|value| !value.is_empty()).map_or_else(
        || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/state/capacity-mode"),
        PathBuf::from,
    )
}

pub fn read(dir: &Path) -> State {
    fs::read(dir.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn write(dir: &Path, state: &State) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join("state.json");
    let temporary = dir.join(format!("state.json.tmp{}", std::process::id()));
    let text = serde_json::to_string(state).map_err(|error| error.to_string())?;
    fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, &path)).map_err(|error| format!("{}: {error}", path.display()))
}

fn record(dir: &Path, adjustment: &Adjustment) {
    let line = format!("{}\n", json!(adjustment));
    let written = fs::OpenOptions::new().create(true).append(true).open(dir.join("adjustments.jsonl")).and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        eprintln!("north capacity mode: {}: {error}", dir.join("adjustments.jsonl").display());
    }
}

pub fn confined_agent_cpus(host: &Host) -> Option<u32> {
    let state = read(&state_dir());
    confinement(Limits::bundled().effects(state.mode), agent_load::game_reserve_on(host), enforced_split().as_ref())
}

pub fn confinement(effects: Effects, reserve_on: bool, split: Option<&game_reserve::Split>) -> Option<u32> {
    let split = split.filter(|_| reserve_on && !effects.pause_heavy)?;
    Some(game_reserve::cpu_count(&split.agent_cpus))
}

fn avg10(path: &Path) -> Option<f64> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().find(|line| line.starts_with("some "))?;
    line.split_whitespace().find_map(|field| field.strip_prefix("avg10="))?.parse().ok()
}

fn protected_psi(cgroup: &Path) -> Option<f64> {
    ["session.slice", "native.slice"].iter().filter_map(|slice| avg10(&cgroup.join(slice).join("cpu.pressure"))).reduce(f64::max)
}

fn host_cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX))
}

pub struct Applied {
    pub effects: Effects,
    pub lines: Vec<String>,
    pub notice: Option<String>,
}

pub fn tick(host: &Host, seat_idle: Option<bool>) -> Applied {
    let limits = Limits::bundled();
    let mut reserve = game_reserve::Settings::from_env();
    let dir = state_dir();
    let previous = read(&dir);
    let seen = Observation {
        now: now(),
        game_running: game_reserve::playing(&reserve),
        seat_idle,
        protected_psi: protected_psi(&reserve.cgroup),
    };
    let (state, stepped) = step(&previous, &seen, &limits);
    let adjustment = stepped.adjustment;
    let mut lines = Vec::new();
    if let Some(adjustment) = &adjustment {
        record(&dir, adjustment);
        lines.push(format!(
            "capacity mode: play reserve {} -> {} CPUs at protected CPU pressure {:.1}%",
            adjustment.from, adjustment.to, adjustment.protected_cpu_some_avg10
        ));
    }
    if state.mode != previous.mode {
        lines.push(format!("capacity mode: {} -> {}: {}", previous.mode.name(), state.mode.name(), state.reason));
    }
    if let Err(error) = write(&dir, &state) {
        eprintln!("north capacity mode: {error}");
    }
    let effects = limits.effects(state.mode);
    if let Some(line) = apply(host, &mut reserve, &state, &limits, effects, adjustment.is_some()) {
        lines.push(line);
    }
    Applied { effects: effects.confined(game_reserve::enforced_split(&reserve).is_some()), lines, notice: stepped.notice }
}

fn apply(host: &Host, reserve: &mut game_reserve::Settings, state: &State, limits: &Limits, effects: Effects, resized: bool) -> Option<String> {
    if agent_load::gaming_marked(&host.capacity) != effects.hold
        && let Err(error) = agent_load::declare_gaming(&host.capacity, effects.hold)
    {
        eprintln!("north capacity mode: {}: {error}", host.capacity.display());
    }
    let on = agent_load::game_reserve_on(host);
    if effects.reserve {
        let (game, agents) = cpu_split(host_cpus(), state.play_cpus.unwrap_or(limits.play_cpus));
        if !agents.is_empty() {
            reserve.game_cpus = game;
            reserve.agent_cpus = agents;
        }
    }
    let outcome = if !effects.reserve {
        if on { game_reserve::off(reserve).map(Some) } else { Ok(None) }
    } else {
        game_reserve::on(reserve, true).map(|line| (!on || resized).then_some(line))
    };
    match outcome {
        Ok(line) => line,
        Err(error) => {
            eprintln!("north capacity mode: game-reserve: {error}");
            None
        }
    }
}

pub fn enforced_split() -> Option<game_reserve::Split> {
    game_reserve::enforced_split(&game_reserve::Settings::from_env())
}

pub fn status_line(state: &State, limits: &Limits, split: Option<&game_reserve::Split>) -> String {
    let effects = limits.effects(state.mode).confined(split.is_some());
    let since = if state.since == 0 { "never ticked".to_owned() } else { format!("since {}", clock(state.since)) };
    let mut line = format!("mode {} ({since}): {}", state.mode.name(), state.reason);
    if effects.reserve {
        line.push_str(&format!("; play reserve {} CPUs", state.play_cpus.unwrap_or(limits.play_cpus)));
    }
    let mut applied = Vec::new();
    if effects.reserve {
        applied.push("game CPUs reserved");
    }
    if effects.hold {
        applied.push("native, exclusive and gpu batch held");
    }
    let confined = split.filter(|_| effects.reserve && !effects.pause_heavy).map(|split| format!("heavy and batch work confined to agent CPUs {}", split.agent_cpus));
    if let Some(confined) = &confined {
        applied.push(confined);
    }
    if effects.pause_heavy {
        applied.push(if effects.reserve { "heavy batch paused until the CPU split is enforced" } else { "heavy batch paused" });
    }
    if applied.is_empty() {
        applied.push("no reservation; agents run at their slice weights");
    }
    format!("{line}; {}", applied.join(", "))
}

pub fn run(arguments: &[String]) -> u8 {
    let limits = Limits::bundled();
    let dir = state_dir();
    let state = read(&dir);
    let changed = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status"] => {
            println!("{}", status_line(&state, &limits, enforced_split().as_ref()));
            return 0;
        }
        ["tick"] => {
            let host = Host::from_env();
            let applied = tick(&host, super::laptop_auto::seat_idle());
            applied.lines.iter().chain(&applied.notice).for_each(|line| println!("{line}"));
            println!("{}", status_line(&read(&dir), &limits, enforced_split().as_ref()));
            return 0;
        }
        ["hold"] => hold(&state, now(), "north capacity mode hold", limits.lease_minutes, &limits),
        ["hold", phrase] => hold(&state, now(), phrase, limits.lease_minutes, &limits),
        ["hold", "--minutes", minutes, rest @ ..] if rest.len() <= 1 => {
            let Ok(minutes) = minutes.parse::<u64>() else {
                eprintln!("{USAGE}");
                return 2;
            };
            hold(&state, now(), rest.first().copied().unwrap_or("north capacity mode hold"), minutes, &limits)
        }
        ["extend"] => match extend(&state, now(), &limits) {
            Some(extended) => extended,
            None => {
                eprintln!("north capacity mode: no playtest lease is on to extend");
                return 1;
            }
        },
        ["release"] => release(&state, now()),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match write(&dir, &changed) {
        Ok(()) => {
            println!("{}", status_line(&changed, &limits, enforced_split().as_ref()));
            0
        }
        Err(error) => {
            eprintln!("north capacity mode: {error}");
            1
        }
    }
}
