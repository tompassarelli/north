use std::fs;
use std::path::Path;

use super::agent_load::{self, Host};
use super::game_reserve;
use super::mode::{Effects, Heat, Limits, Observation, State, cpu_split, enforced_split, now, read, record, state_dir, step, write};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Power {
    pub battery: Option<u8>,
    pub cpu_celsius: Option<f64>,
}

pub fn heat_step(state: &State, celsius: Option<f64>, now: u64, limits: &Limits) -> (Vec<Heat>, bool) {
    let Some(celsius) = celsius else {
        return (Vec::new(), false);
    };
    let mut window = state.heat.iter().copied().filter(|sample| now.saturating_sub(sample.at) < limits.heat_average_seconds).collect::<Vec<_>>();
    window.push(Heat { at: now, celsius });
    let average = window.iter().map(|sample| sample.celsius).sum::<f64>() / window.len() as f64;
    let hot = average >= limits.hot_celsius || (state.hot && average >= limits.cool_celsius);
    (window, hot)
}

pub fn agent_quota_percent(effects: Effects, battery: Option<u8>, hot: bool, relief: Option<u32>, limits: &Limits) -> u32 {
    let mode_quota = if effects.build_host { limits.build_host_quota_percent } else { limits.agent_quota_percent };
    [battery.map(|_| limits.battery_quota_percent), hot.then_some(limits.hot_quota_percent), relief].into_iter().flatten().fold(mode_quota, u32::min)
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

pub fn tick(host: &Host, seat_idle: Option<bool>, power: Power) -> Applied {
    tick_in(host, seat_idle, power, game_reserve::Settings::from_env(), &state_dir(), now())
}

pub fn tick_in(host: &Host, seat_idle: Option<bool>, power: Power, mut reserve: game_reserve::Settings, dir: &Path, now: u64) -> Applied {
    let limits = Limits::bundled();
    let previous = read(dir);
    let seen = Observation {
        now,
        game_running: game_reserve::playing(&reserve),
        seat_idle,
        protected_psi: protected_psi(&reserve.cgroup),
    };
    let (mut state, stepped) = step(&previous, &seen, &limits);
    let adjustment = stepped.adjustment;
    let mut lines = Vec::new();
    if let Some(adjustment) = &adjustment {
        record(dir, adjustment);
        lines.push(format!(
            "capacity mode: play reserve {} -> {} CPUs at protected CPU pressure {:.1}%",
            adjustment.from, adjustment.to, adjustment.protected_cpu_some_avg10
        ));
    }
    if state.mode != previous.mode {
        lines.push(format!("capacity mode: {} -> {}: {}", previous.mode.name(), state.mode.name(), state.reason));
    }
    let effects = limits.effects(state.mode);
    let relief = state.relief.filter(|relief| relief.until > now).map(|relief| relief.percent);
    state.relief = state.relief.filter(|relief| relief.until > now);
    let (heat, hot) = heat_step(&previous, power.cpu_celsius, now, &limits);
    state.heat = heat;
    state.hot = hot;
    let quota = agent_quota_percent(effects, power.battery, hot, relief, &limits);
    if state.agent_quota_percent != Some(quota) {
        match game_reserve::set_agent_quota(&reserve, quota) {
            Ok(()) => {
                lines.push(format!("capacity mode: agent.slice CPUQuota {quota}%"));
                state.agent_quota_percent = Some(quota);
            }
            Err(error) => eprintln!("north capacity mode: agent.slice CPUQuota {quota}%: {error}"),
        }
    }
    if let Err(error) = write(dir, &state) {
        eprintln!("north capacity mode: {error}");
    }
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
