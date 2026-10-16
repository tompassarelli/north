use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::check::{self, Alarms, Courier, Report};
use super::courier::{self, NexusPhone};
use super::dividend;
use super::expect::{self, Expectation, Op, Outcome, State};
use super::facts::{self, Facts, SessionFact, Size, Sources, parse};
use super::snapshot;
use crate::foundation::clock::Timestamp;
use crate::platform::github;

const USAGE: &str = "usage: north ops check [--json] [--local] [--synthetic] [--expectations FILE] | check --replay FIXTURE
       north ops snapshot [--json] [--local] [--expectations FILE]
       north ops dividend [ID] [--json] [--comment] [--expectations FILE]
       north ops deadman --heartbeat FILE [--max-age 15m]
       north ops size [--apply]";
const COMMENT_EVERY: i64 = facts::DAY;
const BRIEFS: &str = ".local/state/agents/handoffs/north-rebuild";

pub(crate) struct Wiring<'a> {
    pub sessions: &'a dyn Fn() -> Vec<SessionFact>,
    pub proxy: &'a dyn Fn(&str) -> Result<(), String>,
}

struct Options {
    json: bool,
    local: bool,
    synthetic: bool,
    comment: bool,
    apply: bool,
    expectations: Option<PathBuf>,
    replay: Option<PathBuf>,
    heartbeat: Option<PathBuf>,
    max_age: Option<String>,
    id: Option<String>,
}

fn options(words: &[&str]) -> Result<Options, String> {
    let mut options = Options { json: false, local: false, synthetic: false, comment: false, apply: false, expectations: None, replay: None, heartbeat: None, max_age: None, id: None };
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let mut value = || words.next().map(|value| value.to_string()).ok_or_else(|| format!("{word} needs a value"));
        match *word {
            "--json" => options.json = true,
            "--local" => options.local = true,
            "--synthetic" => options.synthetic = true,
            "--comment" => options.comment = true,
            "--apply" => options.apply = true,
            "--expectations" => options.expectations = Some(PathBuf::from(value()?)),
            "--replay" => options.replay = Some(PathBuf::from(value()?)),
            "--heartbeat" => options.heartbeat = Some(PathBuf::from(value()?)),
            "--max-age" => options.max_age = Some(value()?),
            id if !id.starts_with('-') && options.id.is_none() => options.id = Some(id.to_owned()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

pub(crate) fn run(arguments: &[String], wiring: &Wiring) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let outcome = match words.split_first() {
        Some((verb, rest)) => options(rest).and_then(|options| match *verb {
            "check" => check(&options, wiring),
            "snapshot" => snapshot(&options, wiring),
            "dividend" => dividends(&options, wiring),
            "deadman" => deadman(&options),
            "size" => size(&options).map(|()| 0),
            _ => Err(USAGE.to_owned()),
        }),
        None => Err(USAGE.to_owned()),
    };
    outcome.unwrap_or_else(|message| {
        eprintln!("north ops: {message}");
        2
    })
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn ops_dir() -> PathBuf {
    home().join(".local/state/north/ops")
}

fn expectations(options: &Options) -> Result<Vec<Expectation>, String> {
    let path = options.expectations.clone().unwrap_or_else(|| home().join(".local/state/north/expectations.toml"));
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    expect::load(&text)
}

fn gather(options: &Options, wiring: &Wiring) -> Facts {
    let sources = Sources::from_home(&home(), !options.local);
    facts::gather(&sources, (wiring.sessions)(), Timestamp::now().unix_seconds())
}

pub(crate) fn replay(fixture: &str) -> Result<Outcome, String> {
    let fixture: Value = serde_json::from_str(fixture).map_err(|error| error.to_string())?;
    let expectation: Expectation = serde_json::from_value(fixture["expectation"].clone()).map_err(|error| format!("expectation: {error}"))?;
    let facts: Facts = serde_json::from_value(fixture["facts"].clone()).map_err(|error| format!("facts: {error}"))?;
    Ok(expect::evaluate(&expectation, &facts))
}

fn check(options: &Options, wiring: &Wiring) -> Result<u8, String> {
    if let Some(path) = &options.replay {
        let outcome = replay(&fs::read_to_string(path).map_err(|error| error.to_string())?)?;
        println!("{}", outcome.line());
        return Ok(u8::from(outcome.state == State::Breached));
    }
    let courier = NexusPhone { proxy: wiring.proxy };
    if options.synthetic {
        return synthetic(&courier);
    }
    let facts = gather(options, wiring);
    let outcomes = match expectations(options) {
        Ok(expectations) => expectations.iter().map(|expectation| expect::evaluate(expectation, &facts)).collect(),
        Err(error) => vec![broken_file(error)],
    };
    let state = ops_dir();
    let mut alarms: Alarms = read_json(&state.join("alarms.json")).unwrap_or_default();
    let mut report = check::sound(&outcomes, &mut alarms, &courier, facts.now);
    write_json(&state.join("alarms.json"), &alarms)?;
    beat(&state, facts.now, &outcomes, &mut report);
    if !options.local && let Err(error) = size_issues(true) {
        report.failures.push(format!("sizing: {error}"));
    }
    print_check(options, &outcomes, &report, &facts);
    Ok(u8::from(!report.breached.is_empty()))
}

fn broken_file(error: String) -> Outcome {
    Outcome {
        id: "expectations-file".into(),
        behavior: format!("the expectations file loads ({error})"),
        metric: "expectations_loaded".into(),
        value: Some(0.0),
        op: Op::Equals,
        threshold: 1.0,
        window: "now".into(),
        state: State::Breached,
        owner: "ops-observability".into(),
        fix: "north ops check refuses an invalid expectations file at edit time".into(),
    }
}

fn synthetic(courier: &dyn Courier) -> Result<u8, String> {
    let now = Timestamp::now().unix_seconds();
    let mut breach = broken_file(String::new());
    breach.id = "synthetic-weekly".into();
    breach.behavior = "weekly synthetic breach: the push path reaches Tom's phone and the proxy".into();
    let mut alarms = Alarms::new();
    let raised = check::sound(std::slice::from_ref(&breach), &mut alarms, courier, now);
    breach.state = State::Holds;
    let cleared = check::sound(std::slice::from_ref(&breach), &mut alarms, courier, now);
    let delivered = raised.failures.is_empty() && cleared.failures.is_empty() && raised.deliveries.len() == 1 && cleared.deliveries.len() == 1;
    let line = format!("{now}\t{}\t{}\t{}\n", u8::from(delivered), raised.deliveries.join(","), cleared.deliveries.join(","));
    append(&ops_dir().join("synthetic.tsv"), &line)?;
    println!("synthetic breach: {} (deliveries {:?} then {:?}; failures {:?})", if delivered { "delivered" } else { "FAILED" }, raised.deliveries, cleared.deliveries, [raised.failures, cleared.failures].concat());
    Ok(u8::from(!delivered))
}

fn beat(state: &Path, now: i64, outcomes: &[Outcome], report: &mut Report) {
    let count = |wanted: State| outcomes.iter().filter(|outcome| outcome.state == wanted).count();
    let line = format!("{now}\t{}\t{}\t{}\n", count(State::Breached), count(State::Unknown), count(State::Holds));
    if let Err(error) = append(&state.join("checks.tsv"), &line).and_then(|()| write(&state.join("heartbeat"), &format!("{now}\n"))) {
        report.failures.push(format!("heartbeat: {error}"));
    }
    if let Err(error) = courier::beat_on_nexus(now) {
        report.failures.push(format!("nexus heartbeat: {error}"));
    }
}

fn print_check(options: &Options, outcomes: &[Outcome], report: &Report, facts: &Facts) {
    if options.json {
        println!("{}", json!({"at": Timestamp::from_unix_seconds(facts.now).iso(), "outcomes": outcomes, "report": report, "gaps": facts.gaps}));
        return;
    }
    outcomes.iter().for_each(|outcome| println!("{}", outcome.line()));
    for (label, items) in [("raised", &report.raised), ("cleared", &report.cleared), ("failures", &report.failures), ("gaps", &facts.gaps)] {
        if !items.is_empty() {
            println!("{label}: {}", items.join("; "));
        }
    }
}

fn snapshot(options: &Options, wiring: &Wiring) -> Result<u8, String> {
    let facts = gather(options, wiring);
    let snapshot = snapshot::assemble(&facts, &expectations(options).unwrap_or_default());
    let text = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    write(&ops_dir().join("snapshot.json"), &text)?;
    write(&ops_dir().join("index.html"), &super::page::render(&snapshot))?;
    match options.json {
        true => println!("{text}"),
        false => println!("{}", snapshot::summary(&snapshot)),
    }
    Ok(0)
}

fn dividends(options: &Options, wiring: &Wiring) -> Result<u8, String> {
    let expectations = expectations(options)?;
    let facts = gather(options, wiring);
    let chosen: Vec<_> = match &options.id {
        Some(id) => vec![dividend::evaluate(expectations.iter().find(|expectation| expectation.id == *id).ok_or_else(|| format!("no expectation named {id}"))?, &facts)],
        None => dividend::linked(&expectations, &facts),
    };
    let unknown = dividend::unknown_ids(&expectations, &facts);
    if options.json {
        println!("{}", json!({"dividends": chosen, "unknown": unknown}));
    } else {
        chosen.iter().for_each(|dividend| println!("{} [{}]", dividend.line(), dividend.issues.join(", ")));
        unknown.iter().for_each(|(issue, id)| println!("{issue}: Dividend names {id}, but no expectation has that id"));
        for issue in facts.bet_issues.iter().flatten().filter(|issue| issue.open && issue.dividends.is_empty()) {
            println!("{}: bet issue has no `north ops dividend <id>` check ({})", issue.reference, issue.title);
        }
    }
    if options.comment {
        comment_due(&chosen, Timestamp::now().unix_seconds())?;
    }
    Ok(u8::from(chosen.iter().any(|dividend| dividend.verdict == dividend::Verdict::Decayed)))
}

fn comment_due(chosen: &[dividend::Dividend], now: i64) -> Result<(), String> {
    let path = ops_dir().join("dividend-comments.json");
    let mut last: BTreeMap<String, i64> = read_json(&path).unwrap_or_default();
    let issues: BTreeSet<&String> = chosen.iter().flat_map(|dividend| &dividend.issues).collect();
    for issue in issues {
        if last.get(issue).is_some_and(|at| now - at < COMMENT_EVERY) {
            continue;
        }
        let (repo, number) = issue.split_once('#').ok_or_else(|| format!("bad issue reference {issue}"))?;
        let mine: Vec<&dividend::Dividend> = chosen.iter().filter(|dividend| dividend.issues.contains(issue)).collect();
        let body = dividend::comment(issue, &mine, &Timestamp::from_unix_seconds(now).iso_minute());
        let full = format!("tompassarelli/{repo}");
        let output = github::run(&["issue", "comment", number, "-R", &full, "--body", &body]).map_err(|error| error.to_string())?;
        if output.status.success() {
            last.insert(issue.clone(), now);
        }
    }
    write_json(&path, &last)
}

fn deadman(options: &Options) -> Result<u8, String> {
    let heartbeat = options.heartbeat.clone().ok_or("deadman needs --heartbeat FILE")?;
    let max_age = expect::window_seconds(Some(options.max_age.as_deref().unwrap_or("15m")));
    let now = Timestamp::now().unix_seconds();
    let beat = fs::read_to_string(&heartbeat).ok().and_then(|text| text.split_whitespace().next()?.parse::<i64>().ok());
    let age_minutes = beat.map(|at| (now - at).max(0) as f64 / 60.0);
    let mut outcome = broken_file(String::new());
    outcome.id = "laptop-heartbeat".into();
    outcome.behavior = format!("the laptop's north ops check wrote {} within {} min", heartbeat.display(), max_age / 60);
    outcome.metric = "heartbeat_age_minutes".into();
    outcome.value = age_minutes;
    outcome.op = Op::AtMost;
    outcome.threshold = (max_age / 60) as f64;
    outcome.window = "now".into();
    outcome.state = if age_minutes.is_some_and(|age| age <= outcome.threshold) { State::Holds } else { State::Breached };
    outcome.fix = "the laptop check timer restarts itself; Nexus keeps paging until it does".into();
    let state = heartbeat.with_extension("alarms.json");
    let mut alarms: Alarms = read_json(&state).unwrap_or_default();
    let report = check::sound(std::slice::from_ref(&outcome), &mut alarms, &PhoneOnly, now);
    write_json(&state, &alarms)?;
    println!("{}", outcome.line());
    report.failures.iter().for_each(|failure| eprintln!("north ops deadman: {failure}"));
    Ok(u8::from(outcome.state == State::Breached))
}

struct PhoneOnly;

impl Courier for PhoneOnly {
    fn phone(&self, title: &str, text: &str) -> Result<String, String> {
        courier::phone("urgent", title, text)
    }

    fn proxy(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

fn size(options: &Options) -> Result<(), String> {
    size_issues(options.apply).map(|lines| lines.iter().for_each(|line| println!("{line}")))
}

fn size_issues(apply: bool) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let cache_path = ops_dir().join("sized.json");
    let mut sized: BTreeSet<String> = read_json(&cache_path).unwrap_or_default();
    let briefs = fs::read_dir(home().join(BRIEFS)).map_err(|error| error.to_string())?;
    let plan: BTreeMap<String, Size> = briefs
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| name.starts_with("brief-") && name.ends_with(".md")))
        .filter_map(|entry| parse::brief_size(&fs::read_to_string(entry.path()).ok()?))
        .filter(|(reference, _)| !sized.contains(reference))
        .collect();
    for (reference, size) in plan {
        let applied = apply && label_issue(&reference, size)?;
        lines.push(format!("{reference} {} {}", size.label(), if applied { "applied" } else { "skipped" }));
        if apply {
            sized.insert(reference);
        }
    }
    write_json(&cache_path, &sized).map(|()| lines)
}

fn label_issue(reference: &str, size: Size) -> Result<bool, String> {
    let (repo, number) = reference.split_once('#').ok_or("bad reference")?;
    let full = format!("tompassarelli/{repo}");
    let issue = github::json(&["issue", "view", number, "-R", &full, "--json", "state,labels,milestone"])?;
    let labels: Vec<&str> = issue["labels"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
    let tracked = labels.contains(&"bet") || issue["milestone"]["title"] == "Public 0.1";
    if issue["state"] != "OPEN" || !tracked || Size::from_labels(labels.iter().copied()).is_some() {
        return Ok(false);
    }
    let _ = github::run(&["label", "create", size.label(), "-R", &full, "--color", "c5def5", "--description", "Box size from its brief's ETA: S <= 60 min, M <= 180 min, L longer"]);
    let output = github::run(&["issue", "edit", number, "-R", &full, "--add-label", size.label()]).map_err(|error| error.to_string())?;
    Ok(output.status.success())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write(path, &serde_json::to_string_pretty(value).map_err(|error| error.to_string())?)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, path)).map_err(|error| format!("{}: {error}", path.display()))
}

fn append(path: &Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    fs::create_dir_all(path.parent().ok_or("no parent directory")?).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

