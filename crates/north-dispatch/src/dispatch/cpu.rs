//! The cgroup units that used the most CPU on this dispatch host over a short sample: a dry run prints them when the host's one-minute load is over twice its cores, and a placing round pages the coordinator with them once per overload.
use super::effects;
use super::record;
use super::slots::Host;
use super::survey::Survey;
use super::tasks::{leaves, DEPTH, SLICES};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

const CGROUPS: &str = "/sys/fs/cgroup";
const SAMPLE: Duration = Duration::from_secs(5);
const TOP: usize = 10;
const KEY: &str = "dispatch-cpu-overload";
const STATE: &str = "cpu-overload.json";

fn overloaded(survey: &Survey) -> Option<&Host> {
    survey.hosts.iter().find(|host| host.name == survey.here).filter(|host| host.measure.is_some_and(|measure| measure.load > 2.0 * measure.cores))
}

/// Prints the ten largest CPU consumers by unit name, largest first, each with the cores it used in the sample and its process count. Only names are printed, never a command line, environment or path.
pub(super) fn print(survey: &Survey) {
    if let Some(host) = overloaded(survey) {
        for line in lines(&host.name) {
            println!("{line}");
        }
    }
}

/// Pages the coordinator once when this host goes over twice its cores, with the load, the idle cores and the same unit lines the dry run prints, and once more when the load falls back. The keyed send holds the page open for the whole overload, so the coordinator hears of one episode at most once.
pub(super) fn page(survey: &Survey, at: f64) {
    let Some(host) = survey.hosts.iter().find(|host| host.name == survey.here) else { return };
    let Some(measure) = host.measure else { return };
    let open = std::fs::read(record::directory().join(STATE)).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).is_some_and(|state| state["open"] == true);
    let over = measure.load > 2.0 * measure.cores;
    if over && !open {
        open_episode(host, &survey.live, at);
    } else if !over && open {
        close_episode(host, at);
    }
}

fn open_episode(host: &Host, live: &std::collections::BTreeSet<String>, at: f64) {
    let Some(measure) = host.measure else { return };
    let idle = measure.idle_cores.map_or_else(|| "unread".to_owned(), |idle| format!("{idle:.1}"));
    let summary = format!("north fleet dispatch on {}: load {:.1} on {:.0} cores, {idle} idle cores", host.name, measure.load, measure.cores);
    let text = std::iter::once(format!("{summary}; the CPU consumers:")).chain(lines(&host.name)).chain([super::waits::slowest(&host.name, at)]).chain(super::tasks::lines(&host.name, live)).collect::<Vec<_>>().join("\n");
    match effects::page_condition(KEY, &text) {
        Ok(()) => effects::remember(STATE, &json!({"open": true, "at": at})),
        Err(why) => eprintln!("north fleet dispatch: CPU overload not paged: {why}"),
    }
}

fn close_episode(host: &Host, at: f64) {
    let Some(measure) = host.measure else { return };
    let text = format!("north fleet dispatch on {}: load {:.1} on {:.0} cores; the CPU overload cleared", host.name, measure.load, measure.cores);
    match effects::clear_condition(KEY, &text) {
        Ok(()) => effects::remember(STATE, &json!({"open": false, "at": at})),
        Err(why) => eprintln!("north fleet dispatch: CPU overload clearing not paged: {why}"),
    }
}

fn lines(name: &str) -> Vec<String> {
    let rows = sample();
    if rows.is_empty() {
        return vec![format!("host {name} cpu: not read: no cgroup CPU counters under {CGROUPS}")];
    }
    rows.into_iter().map(|(unit, cores, procs)| format!("host {name} cpu: {unit} {cores:.2} cores, {procs} processes")).collect()
}

fn sample() -> Vec<(String, f64, usize)> {
    let mut units = Vec::new();
    for slice in SLICES {
        leaves(&Path::new(CGROUPS).join(slice), DEPTH, &mut units);
    }
    let before: Vec<Option<u64>> = units.iter().map(|unit| usage_usec(unit)).collect();
    let started = Instant::now();
    std::thread::sleep(SAMPLE);
    let window_usec = started.elapsed().as_micros() as f64;
    let mut rows: Vec<(String, f64, usize)> = units
        .iter()
        .zip(before)
        .filter_map(|(unit, before)| {
            let used = usage_usec(unit)?.checked_sub(before?)?;
            Some((unit.file_name()?.to_string_lossy().into_owned(), used as f64 / window_usec, procs(unit)))
        })
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    rows.truncate(TOP);
    rows
}

fn usage_usec(unit: &Path) -> Option<u64> {
    std::fs::read_to_string(unit.join("cpu.stat")).ok()?.lines().find_map(|line| line.strip_prefix("usage_usec ")?.parse().ok())
}

fn procs(unit: &Path) -> usize {
    std::fs::read_to_string(unit.join("cgroup.procs")).map_or(0, |text| text.lines().count())
}
