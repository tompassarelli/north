use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;

use regex::Regex;
use serde::Deserialize;

use super::Inputs;
use super::sample::read;
use crate::foundation::clock::Timestamp;

const LOW_AVAILABLE_MIB: i64 = 600;
const PSI_FULL_LIMIT: f64 = 1.0;
const STEAL_LIMIT_PCT: f64 = 20.0;
const SUSTAINED_SECONDS: f64 = 600.0;
const UPGRADE_DAYS: usize = 3;
const ROW_SECONDS: f64 = 60.0;
const GAP_SECONDS: f64 = 150.0;
const DAY_SECONDS: f64 = 86_400.0;
const KILL_PATTERNS: [&str; 2] = [
    r"Out of memory: Killed process \d+ \((?P<name>[^)]*)\)",
    r#"sending SIG(?:TERM|KILL) to process \d+ uid \d+ "(?P<name>[^"]*)""#,
];

#[derive(Debug, Deserialize)]
pub(super) struct Row {
    ts: String,
    mem_available_mib: i64,
    swap_used_mib: i64,
    zram_used_mib: i64,
    zram_compressed_mib: i64,
    psi_some_avg60: f64,
    psi_full_avg60: f64,
    agents: i64,
    agent_pss_mib: i64,
    #[serde(default)]
    gap_s: Option<i64>,
    #[serde(default)]
    steal_pct: Option<f64>,
    #[serde(skip)]
    epoch: f64,
}

struct Kill {
    stamp: String,
    name: String,
    agent: bool,
}

fn load_rows(inputs: &Inputs, since: f64, until: f64) -> Vec<Row> {
    let first = Timestamp::from_unix_seconds_f64(since).unix_days();
    let last = Timestamp::from_unix_seconds_f64(until).unix_days();
    let mut rows = (first..=last)
        .flat_map(|day| {
            let date = Timestamp::from_unix_seconds(day * 86_400).iso_date();
            read(&super::day_file(&inputs.dir, &date)).lines().filter_map(parse_row).collect::<Vec<_>>()
        })
        .filter(|row| since <= row.epoch && row.epoch <= until)
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.epoch.total_cmp(&b.epoch));
    rows
}

fn parse_row(line: &str) -> Option<Row> {
    let mut row = serde_json::from_str::<Row>(line).ok()?;
    row.epoch = Timestamp::parse(&row.ts).ok()?.unix_seconds() as f64;
    Some(row)
}

fn percentile(values: &[i64], p: f64) -> i64 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let index = (p / 100.0 * ordered.len() as f64).ceil() as usize;
    ordered[index.saturating_sub(1)]
}

fn longest_run(rows: &[&Row], hot: impl Fn(&Row) -> bool) -> f64 {
    let (mut best, mut start, mut last) = (0.0_f64, None, 0.0);
    for row in rows {
        if !hot(row) {
            start = None;
            continue;
        }
        let begun = match start {
            Some(begun) if row.epoch - last <= GAP_SECONDS => begun,
            _ => row.epoch,
        };
        start = Some(begun);
        last = row.epoch;
        best = best.max(last - begun + ROW_SECONDS);
    }
    best
}

fn sustained_days(rows: &[Row]) -> Vec<(String, f64, f64)> {
    let mut by_day = BTreeMap::<&str, Vec<&Row>>::new();
    for row in rows {
        by_day.entry(&row.ts[..10]).or_default().push(row);
    }
    by_day
        .into_iter()
        .filter_map(|(day, day_rows)| {
            let low = longest_run(&day_rows, |row| row.mem_available_mib < LOW_AVAILABLE_MIB);
            let full = longest_run(&day_rows, |row| row.psi_full_avg60 > PSI_FULL_LIMIT);
            (low > SUSTAINED_SECONDS || full > SUSTAINED_SECONDS).then(|| (day.to_owned(), low, full))
        })
        .collect()
}

fn journal_lines(inputs: &Inputs, since: f64, until: f64) -> Vec<String> {
    let text = match &inputs.journal {
        Some(path) => read(path),
        None => Command::new("journalctl")
            .args(["--no-pager", "-q", "-o", "short-iso"])
            .args(["--since", &format!("@{}", since as i64), "--until", &format!("@{}", until as i64 + 1)])
            .args(["-g", "Out of memory: Killed process|sending SIG(TERM|KILL) to process"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default(),
    };
    text.lines().map(str::to_owned).collect()
}

fn oom_kills(inputs: &Inputs, since: f64, until: f64) -> Vec<Kill> {
    let patterns = KILL_PATTERNS.map(|pattern| Regex::new(pattern).expect("kill pattern"));
    let agent_name = Regex::new("claude|codex").expect("agent pattern");
    let mut kills = Vec::new();
    for line in journal_lines(inputs, since, until) {
        let stamp = line.split_whitespace().next().unwrap_or("").to_owned();
        if let Some(at) = Timestamp::parse_python_datetime(&stamp).map(Timestamp::unix_seconds_f64)
            && !(since <= at && at <= until)
        {
            continue;
        }
        if let Some(found) = patterns.iter().find_map(|pattern| pattern.captures(&line)) {
            let whole = found.get(0).expect("whole match");
            let agent = agent_name.is_match(&line[whole.start()..]);
            kills.push(Kill { stamp, name: found["name"].to_owned(), agent });
        }
    }
    kills
}

fn longest_gap(rows: &[Row]) -> Option<(i64, &str)> {
    let between = rows.windows(2).map(|pair| ((pair[1].epoch - pair[0].epoch) as i64, pair[1].ts.as_str()));
    let recorded = rows.iter().filter_map(|row| Some((row.gap_s?, row.ts.as_str())));
    between.chain(recorded).fold(None, |best, gap| match best {
        Some(best @ (seconds, _)) if seconds >= gap.0 => Some(best),
        _ => Some(gap),
    })
}

fn max_of(rows: &[Row], field: impl Fn(&Row) -> i64) -> i64 {
    rows.iter().map(field).max().unwrap_or(0)
}

fn cpu_lines(rows: &[Row], out: &mut impl Write) -> std::io::Result<()> {
    let sampled = rows.iter().filter_map(|row| row.steal_pct).collect::<Vec<_>>();
    let stolen = sampled.iter().filter(|steal| **steal > STEAL_LIMIT_PCT).count();
    writeln!(out, "minutes with steal > 20%: {stolen} (of {} rows with steal sampled)", sampled.len())?;
    match longest_gap(rows) {
        Some((seconds, ts)) => writeln!(out, "longest row gap: {seconds} s ending {ts}"),
        None => writeln!(out, "longest row gap: none"),
    }
}

pub(super) fn report(inputs: &Inputs, days: f64, out: &mut impl Write) -> u8 {
    let until = inputs.now;
    let since = until - days * DAY_SECONDS;
    let rows = load_rows(inputs, since, until);
    let kills = oom_kills(inputs, since, until);
    match write_report(&rows, &kills, days, inputs, out) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("nexus-ram: {error}");
            1
        }
    }
}

fn write_report(rows: &[Row], kills: &[Kill], days: f64, inputs: &Inputs, out: &mut impl Write) -> std::io::Result<u8> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        writeln!(out, "nexus-ram: no rows in the last {days} days in {}", inputs.dir.display())?;
        writeln!(out, "verdict: STAY (no data)")?;
        return Ok(1);
    };
    let agents = rows.iter().map(|row| row.agent_pss_mib).collect::<Vec<_>>();
    let counts = rows.iter().map(|row| row.agents).collect::<Vec<_>>();
    let span_days = (last.epoch - first.epoch + ROW_SECONDS) / DAY_SECONDS;
    let full_minutes = rows.iter().filter(|row| row.psi_full_avg60 > PSI_FULL_LIMIT).count();
    let some_minutes = rows.iter().filter(|row| row.psi_some_avg60 > PSI_FULL_LIMIT).count();
    let pressure_days = sustained_days(rows);
    let agent_kills = kills.iter().filter(|kill| kill.agent).count();
    writeln!(out, "window: last {days} days, {} rows covering {span_days:.2} days ({} .. {})", rows.len(), first.ts, last.ts)?;
    writeln!(out, "agent CLIs: p50 {} p95 {} max {}", percentile(&counts, 50.0), percentile(&counts, 95.0), max_of(rows, |row| row.agents))?;
    writeln!(out, "agent PSS MiB: p50 {} p95 {} max {}", percentile(&agents, 50.0), percentile(&agents, 95.0), max_of(rows, |row| row.agent_pss_mib))?;
    writeln!(out, "min MemAvailable MiB: {}", rows.iter().map(|row| row.mem_available_mib).min().unwrap_or(0))?;
    writeln!(
        out,
        "max swap used MiB: {}; max zram used MiB: {}; max zram compressed MiB: {}",
        max_of(rows, |row| row.swap_used_mib),
        max_of(rows, |row| row.zram_used_mib),
        max_of(rows, |row| row.zram_compressed_mib)
    )?;
    writeln!(out, "minutes with PSI full avg60 > 1%: {full_minutes} (some: {some_minutes})")?;
    cpu_lines(rows, out)?;
    writeln!(out, "OOM kills: {} ({agent_kills} hit an agent)", kills.len())?;
    for kill in kills {
        writeln!(out, "  {} {}{}", kill.stamp, kill.name, if kill.agent { " [agent]" } else { "" })?;
    }
    for (day, low, full) in &pressure_days {
        writeln!(out, "sustained pressure {day}: MemAvailable < {LOW_AVAILABLE_MIB} MiB for {} min, PSI full > 1% for {} min", (low / 60.0) as i64, (full / 60.0) as i64)?;
    }
    let mut reasons = Vec::new();
    if pressure_days.len() >= UPGRADE_DAYS {
        reasons.push(format!("{} days with sustained pressure", pressure_days.len()));
    }
    if agent_kills > 0 {
        reasons.push(format!("{agent_kills} OOM kills hit an agent"));
    }
    if reasons.is_empty() {
        writeln!(out, "verdict: STAY ({} days with sustained pressure, {agent_kills} agent OOM kills)", pressure_days.len())?;
    } else {
        writeln!(out, "verdict: UPGRADE ({})", reasons.join("; "))?;
    }
    Ok(0)
}
