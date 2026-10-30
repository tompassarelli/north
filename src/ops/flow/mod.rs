//! `north flow`: how fast each repository's lanes land, whether landing has stalled, and which
//! capacity is idle. With `--stall-alarm` it also tells the Fleet parent and the proxy of a stall or a
//! saturated host every fifteen minutes while it lasts, and runs `ci-diagnose`.
use std::time::Duration;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use alerts::stall_alert;
use capacity::saturation_alert;
use guards::actions_verdict;
use host::{Host, Machine, Places, valid_name};
use jobs::account_busy;
use measure::{Env, get, project_targets};
use notes::{brief_coverage, usage_stale};
use render::format_row;

mod alerts;
mod capacity;
mod autoland;
mod goal;
mod guards;
mod host;
mod jobs;
mod measure;
mod notes;
mod refused;
mod render;
mod stall;
mod stranded;
mod throughput;
#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod lanes_tests;
#[cfg(test)]
mod readings_tests;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north flow [--json] [--repo NAME]... [--stall-alarm] | north flow row REPO [FIELD]...";
const FIELDS: [&str; 2] = ["reuse", "closed_per_hour"];
const DEFAULT_REPOS: [&str; 4] = ["smashcraft", "wisp", "north", "smashcraft-client"];
const USAGE_STALE_HOURS: f64 = 7.0;

struct Options {
    json: bool,
    repos: Vec<String>,
    alarm: bool,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { json: false, repos: Vec::new(), alarm: false };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--json" => options.json = true,
            "--stall-alarm" => options.alarm = true,
            "--repo" => options.repos.push(rest.next()?.clone()),
            _ => return None,
        }
    }
    Some(options)
}

fn laptop_line(host: &dyn Host) -> String {
    let Some(output) = host.output(&["north", "laptop", "status"], None, None, Duration::from_secs(20)) else {
        return "laptop: unknown (north laptop status did not answer)".into();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match stdout.trim().lines().next() {
        Some(first) if output.status.success() && first.starts_with("laptop: ") => first.into(),
        _ => "laptop: unknown (north laptop status failed)".into(),
    }
}

fn announce(places: &Places, now: f64, options: &Options) {
    if options.json {
        return;
    }
    if let Some(age) = usage_stale(places, now, USAGE_STALE_HOURS) {
        println!("agents flow: usage cache stale: claude-web.json last written {age:.0}h ago; usage-web-read runs every 6h");
    }
    let covered = brief_coverage(places).map_or("unknown".into(), |covered| covered.to_string());
    println!("agents flow: tagged finished worker briefs {covered}/300 retrieval trigger; backtest requires >500");
}

fn alarm(places: &Places, host: &dyn Host, rows: &[Value], now: f64) {
    if stall_alert(places, host, rows, now).is_err() {
        println!("agents flow: stall alert state error");
    }
    if saturation_alert(places, host, &crate::pace::target::policy_path(), now).is_err() {
        println!("agents flow: saturation alert state error");
    }
    match host.output(&["ci-diagnose"], None, None, Duration::from_secs(1200)) {
        Some(output) => {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        None => println!("agents flow: ci-diagnose error"),
    }
}

fn row(arguments: &[String]) -> u8 {
    let [repo, fields @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if !valid_name(repo) {
        eprintln!("north flow: invalid repository name");
        return 2;
    }
    let (places, machine) = (Places::from_env(), Machine::default());
    let env = Env::new(&places, &machine, project_targets(&crate::pace::target::policy_path()));
    let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
    let row = get(&env, repo, Timestamp::now().unix_seconds_f64(), &fields);
    println!("{}", json!({ "row": row, "line": format_row(&row, None) }));
    0
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.first().map(String::as_str) == Some("row") {
        return row(&arguments[1..]);
    }
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let places = Places::from_env();
    let machine = Machine::default();
    let targets = project_targets(&crate::pace::target::policy_path());
    let repos: Vec<String> = if options.repos.is_empty() {
        DEFAULT_REPOS.iter().map(|repo| (*repo).to_owned()).chain(targets.keys().cloned()).collect()
    } else {
        options.repos.clone()
    };
    let mut seen = Vec::new();
    for repo in repos {
        if !valid_name(&repo) {
            eprintln!("north flow: invalid repository name");
            return 2;
        }
        if !seen.contains(&repo) {
            seen.push(repo);
        }
    }
    let now = Timestamp::now().unix_seconds_f64();
    let env = Env::new(&places, &machine, targets);
    let mut rows: Vec<Value> = seen.iter().map(|repo| get(&env, repo, now, &FIELDS)).collect();
    let samples = account_busy(&places, &machine, now);
    for row in &mut rows {
        row["actions"] = json!(actions_verdict(row["pending"].as_u64(), &samples, now));
    }
    if options.json {
        println!("{}", Value::Array(rows.clone()));
    } else {
        println!("{}", laptop_line(&machine));
        println!("{}", rows.iter().map(|row| format_row(row, None)).collect::<Vec<_>>().join("\n"));
    }
    announce(&places, now, &options);
    if options.alarm {
        alarm(&places, &machine, &rows, now);
    }
    eprintln!("agents flow: {} GitHub API calls this run", machine.github_calls());
    0
}
