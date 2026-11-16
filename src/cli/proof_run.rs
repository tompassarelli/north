//! The proof run's pacer (north#1632). `print` writes one file per issue, its title then its body, with every stand-in
//! box stamped `(not before TIME)` at its stage's rate and dealt round-robin in time order, for an operator to file; `watch`
//! touches no GitHub: it counts the stand-in landings the lander records against the boxes due so far and stops when a
//! box due over five minutes ago has stayed unlanded for five minutes.
use std::process::Command;
use std::time::{Duration, Instant};

use north_foundation::foundation::clock::Timestamp;

const USAGE: &str = "usage: north proof-run print|watch --start YYYY-MM-DDTHH:MMZ [--stages RATE:MINUTES,...] [--dies-at RATE --dies N] [--profile-repo NAME] [--issues N --dir DIRECTORY]";
const STAGES: &str = "30:10,60:10,120:10,210:70";
const LATE: f64 = 300.0;
const READ_EVERY: Duration = Duration::from_mins(1);
const HEADER: &str = "Category: tooling\nSpec: exact\nScope: cross-module\nSurface: shell\nVerify: local-test\n\nStand-in boxes of the north#1632 proof run: each lands one line under lanes/standin/ through a stand-in worker.\n\n## Done when\n";
const LANE_PREFIX: &str = "si";

struct Plan {
    start: i64,
    profile_repo: String,
    stages: Vec<(u32, u32)>,
    dies_at: Option<u32>,
    dies: u32,
    issues: u32,
    directory: Option<std::path::PathBuf>,
}

struct Slot {
    rate: u32,
    index: u32,
    at: i64,
}

pub(super) fn run(arguments: &[String]) -> u8 {
    let (verb, rest) = arguments.split_first().map_or(("", &[][..]), |(verb, rest)| (verb.as_str(), rest));
    let plan = match parse(rest) {
        Ok(plan) => plan,
        Err(why) => {
            eprintln!("north proof-run: {why}\n{USAGE}");
            return 64;
        }
    };
    match verb {
        "print" => print(&plan),
        "watch" => watch(&plan),
        _ => {
            eprintln!("{USAGE}");
            64
        }
    }
}

fn parse(arguments: &[String]) -> Result<Plan, String> {
    let mut plan = Plan { start: 0, profile_repo: "autoland-scratch".into(), stages: Vec::new(), dies_at: None, dies: 0, issues: 30, directory: None };
    let mut stages = STAGES.to_owned();
    let mut words = arguments.iter();
    while let Some(flag) = words.next() {
        let value = words.next().ok_or_else(|| format!("{flag} takes a value"))?.clone();
        match flag.as_str() {
            "--start" => plan.start = Timestamp::parse_loose(&value).ok_or_else(|| format!("--start {value} is no UTC time"))?.unix_seconds(),
            "--profile-repo" => plan.profile_repo = value,
            "--stages" => stages = value,
            "--dies-at" => plan.dies_at = Some(value.parse().map_err(|_| format!("--dies-at {value} is no rate"))?),
            "--dies" => plan.dies = value.parse().map_err(|_| format!("--dies {value} is no count"))?,
            "--issues" => plan.issues = value.parse().map_err(|_| format!("--issues {value} is no count"))?,
            "--dir" => plan.directory = Some(value.into()),
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if plan.start == 0 {
        return Err("--start is required".into());
    }
    for stage in stages.split(',') {
        let number = |text: &str| text.parse::<u32>().ok().filter(|value| *value > 0).ok_or_else(|| format!("stage {stage} is not RATE:MINUTES"));
        let (rate, minutes) = stage.split_once(':').ok_or_else(|| format!("stage {stage} is not RATE:MINUTES"))?;
        plan.stages.push((number(rate)?, number(minutes)?));
    }
    Ok(plan)
}

fn slots(plan: &Plan) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut begin = plan.start;
    for (rate, minutes) in plan.stages.iter().copied() {
        let count = rate * minutes / 60;
        slots.extend((0..count).map(|index| Slot { rate, index: index + 1, at: begin + i64::from(index) * 3600 / i64::from(rate) }));
        begin += i64::from(minutes) * 60;
    }
    slots
}

fn print(plan: &Plan) -> u8 {
    let Some(directory) = plan.directory.as_ref() else {
        eprintln!("north proof-run print: --dir DIRECTORY is required\n{USAGE}");
        return 64;
    };
    let issues = plan.issues.max(1) as usize;
    let mut bodies: Vec<String> = (1..=issues).map(|number| format!("Stand-in boxes {number} of {issues} (north#1632 proof run)\n{HEADER}")).collect();
    for (dealt, slot) in slots(plan).iter().enumerate() {
        let dies = if plan.dies_at == Some(slot.rate) && slot.index <= plan.dies { " dies once mid-lane" } else { "" };
        let minute = Timestamp::from_unix_seconds((slot.at + 59) / 60 * 60).iso_minute();
        bodies[dealt % issues].push_str(&format!("- [ ] standin {}-{:04}{dies} (not before {minute})\n", slot.rate, slot.index));
    }
    if let Err(why) = std::fs::create_dir_all(directory) {
        eprintln!("north proof-run print: {}: {why}", directory.display());
        return 1;
    }
    for (number, body) in bodies.iter().enumerate() {
        let file = directory.join(format!("{:02}.md", number + 1));
        if let Err(why) = std::fs::write(&file, body) {
            eprintln!("north proof-run print: {}: {why}", file.display());
            return 1;
        }
    }
    println!("{} issue files with {} boxes in {}", issues, slots(plan).len(), directory.display());
    0
}

fn watch(plan: &Plan) -> u8 {
    let slots = slots(plan);
    let end = plan.start + plan.stages.iter().map(|(_, minutes)| i64::from(*minutes) * 60).sum::<i64>();
    let mut behind_since: Option<i64> = None;
    loop {
        let now = Timestamp::now().unix_seconds();
        let due = slots.iter().filter(|slot| (slot.at + 59) / 60 * 60 + LATE as i64 <= now).count();
        let opened = slots.iter().filter(|slot| slot.at <= now).count();
        let rate = slots.iter().rev().find(|slot| slot.at <= now).map_or(0, |slot| slot.rate);
        match landed(&plan.profile_repo, plan.start, now) {
            Ok(landed) => {
                println!("t+{}s stage {rate}/h: opened {opened} landed {landed} due {due}", now - plan.start);
                behind_since = if landed < due { behind_since.or(Some(now)) } else { None };
            }
            Err(why) => println!("t+{}s stage {rate}/h: landings unread: {why}", now - plan.start),
        }
        if behind_since.is_some_and(|since| (now - since) as f64 >= LATE) {
            println!("stop: stage {rate}/h fell behind: boxes due over 5 min ago stayed unlanded for 5 min");
            return 2;
        }
        if now > end + LATE as i64 {
            println!("done: the schedule ended at t+{}s", end - plan.start);
            return 0;
        }
        let started = Instant::now();
        while started.elapsed() < READ_EVERY {
            std::thread::sleep(READ_EVERY - started.elapsed());
        }
    }
}

fn landed(profile_repo: &str, start: i64, now: i64) -> Result<usize, String> {
    let window = format!("{}m", ((now - start).max(0) + 119) / 60);
    let output = Command::new(north_base::platform::north_binary::north_binary()).args(["land", "profile", "--since", &window, "--repo", profile_repo]).output().map_err(|why| format!("north land profile: {why}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or_default().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.contains(" landed ") && line.trim_start_matches("- ").starts_with(LANE_PREFIX)).count())
}
