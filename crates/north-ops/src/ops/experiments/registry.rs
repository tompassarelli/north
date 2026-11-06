//! The experiment registry: one entry per live policy or routing change, with the outcome metric it
//! should move, the predicted direction and bar, the windows compared, its review date and its owner.
use serde::Deserialize;

use crate::foundation::clock::parse_day_number;
use crate::platform::toml_json;
use crate::threads::calibration::rework::REWORK_WINDOW_S;

pub const PATH: &str = "docs/rationale/experiments.toml";
const DAY_S: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(pub i64);

impl Day {
    pub fn parse(text: &str) -> Option<Self> {
        parse_day_number(text).map(Self)
    }

    pub fn of_unix(seconds: i64) -> Self {
        Self(seconds.div_euclid(DAY_S))
    }

    pub fn unix(self) -> i64 {
        self.0 * DAY_S
    }

    pub fn text(self) -> String {
        crate::foundation::clock::Timestamp::from_unix_seconds(self.unix()).iso_date()
    }
}

/// Days `[start, end)`, read as the UTC seconds from the start day's midnight to the end day's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: Day,
    pub end: Day,
}

impl Window {
    pub fn text(self) -> String {
        format!("{} to {}", self.start.text(), self.end.text())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Metric {
    ReworkRate,
    UsdPerSurviving,
    FirstCommitP50Minutes,
    Landed,
    Runs,
    IncidentRows(String),
    Unread(String),
}

impl Metric {
    pub const NAMES: [&'static str; 6] = ["rework_rate", "usd_per_surviving", "first_commit_p50_minutes", "landed", "runs", "incident_rows_per_week"];

    fn parse(name: &str, measure: Option<&str>, class: Option<&str>) -> Result<Self, String> {
        match (name, class) {
            ("incident_rows_per_week", Some(class)) if measure.is_none() && !class.trim().is_empty() => return Ok(Self::IncidentRows(class.trim().to_owned())),
            ("incident_rows_per_week", _) => return Err("metric incident_rows_per_week needs a class and no measure".into()),
            (_, Some(_)) => return Err(format!("class is only for metric incident_rows_per_week, not {name}")),
            _ => {}
        }
        match (name, measure) {
            ("rework_rate", None) => Ok(Self::ReworkRate),
            ("usd_per_surviving", None) => Ok(Self::UsdPerSurviving),
            ("first_commit_p50_minutes", None) => Ok(Self::FirstCommitP50Minutes),
            ("landed", None) => Ok(Self::Landed),
            ("runs", None) => Ok(Self::Runs),
            ("none", Some(measure)) if !measure.trim().is_empty() => Ok(Self::Unread(measure.trim().to_owned())),
            ("none", _) => Err("metric none needs a measure naming how the rule is judged".into()),
            (_, Some(_)) => Err(format!("measure is only for metric none, not {name}")),
            (other, None) => Err(format!("unknown metric {other}; north flow's outcome metrics are {} (or none with a measure)", Self::NAMES.join(", "))),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::ReworkRate => "rework_rate",
            Self::UsdPerSurviving => "usd_per_surviving",
            Self::FirstCommitP50Minutes => "first_commit_p50_minutes",
            Self::Landed => "landed",
            Self::Runs => "runs",
            Self::IncidentRows(_) => "incident_rows_per_week",
            Self::Unread(_) => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn sign(self) -> f64 {
        match self {
            Self::Up => 1.0,
            Self::Down => -1.0,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// The worker runs a reading covers: one tier, one arm, both, or every run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub tier: Option<String>,
    pub arm: Option<String>,
}

impl Scope {
    pub fn text(&self) -> String {
        match (&self.tier, &self.arm) {
            (None, None) => "every worker run".into(),
            (Some(tier), None) => format!("tier {tier}"),
            (None, Some(arm)) => format!("arm {arm}"),
            (Some(tier), Some(arm)) => format!("tier {tier} arm {arm}"),
        }
    }
}

/// An issue as `owner/repo#N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub repo: String,
    pub number: u64,
}

impl Issue {
    pub fn parse(text: &str) -> Option<Self> {
        let (repo, number) = text.split_once('#')?;
        let (owner, name) = repo.split_once('/')?;
        let valid = |part: &str| !part.is_empty() && part.chars().all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character));
        (valid(owner) && valid(name)).then_some(())?;
        Some(Self { repo: repo.to_owned(), number: number.parse().ok().filter(|number| *number > 0)? })
    }

    pub fn text(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}

/// The rationale note and the exact review rule an entry carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cited {
    pub note: String,
    pub rule: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub change: String,
    pub landed: String,
    pub cited: Option<Cited>,
    pub metric: Metric,
    pub scope: Scope,
    pub direction: Direction,
    pub bar: f64,
    pub min_n: usize,
    pub before: Option<Window>,
    pub after: Window,
    pub review: Day,
    pub owner: Issue,
}

/// A box whose code landed and whose only remaining check is a measurement over time: the loop runs
/// `command` on `review` and ticks the box on a pass or reopens it with the output on a miss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub text: String,
    pub command: String,
    pub expect: Option<String>,
    pub review: Day,
    pub owner: Issue,
}

#[derive(Debug)]
pub struct Registry {
    pub experiments: Vec<Experiment>,
    pub checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    experiment: Vec<Raw>,
    #[serde(default)]
    check: Vec<RawCheck>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    id: String,
    #[serde(rename = "box")]
    text: String,
    command: String,
    expect: Option<String>,
    review: String,
    owner: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    id: String,
    change: String,
    landed: String,
    note: Option<String>,
    rule: Option<String>,
    metric: String,
    measure: Option<String>,
    class: Option<String>,
    tier: Option<String>,
    arm: Option<String>,
    direction: String,
    bar: f64,
    min_n: usize,
    before: Option<[String; 2]>,
    after: [String; 2],
    review: String,
    owner: String,
}

pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn window(id: &str, field: &str, days: &[String; 2]) -> Result<Window, String> {
    let day = |text: &str| Day::parse(text).ok_or_else(|| format!("{id}: {field} day {text} is not YYYY-MM-DD"));
    let window = Window { start: day(&days[0])?, end: day(&days[1])? };
    (window.start < window.end).then_some(window).ok_or_else(|| format!("{id}: {field} must start before it ends"))
}

fn sha(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.chars().all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

fn well_named(id: &str) -> Result<(), String> {
    if id.is_empty() || !id.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-') {
        return Err(format!("id {id:?} must be lowercase words joined by -"));
    }
    Ok(())
}

fn named(raw: &Raw) -> Result<(), String> {
    let id = &raw.id;
    well_named(id)?;
    if collapse(&raw.change).is_empty() {
        return Err(format!("{id}: change is empty"));
    }
    if !sha(&raw.landed) {
        return Err(format!("{id}: landed {} is not a commit sha", raw.landed));
    }
    Ok(())
}

fn entry(raw: Raw) -> Result<Experiment, String> {
    named(&raw)?;
    let id = raw.id;
    let cited = match (raw.note, raw.rule) {
        (Some(note), Some(rule)) if !collapse(&rule).is_empty() => Some(Cited { note, rule: collapse(&rule) }),
        (None, None) => None,
        _ => return Err(format!("{id}: note and rule come together")),
    };
    let metric = Metric::parse(&raw.metric, raw.measure.as_deref(), raw.class.as_deref()).map_err(|why| format!("{id}: {why}"))?;
    let direction = match raw.direction.as_str() {
        "up" => Direction::Up,
        "down" => Direction::Down,
        other => return Err(format!("{id}: direction {other} is not up or down")),
    };
    if !raw.bar.is_finite() || raw.bar < 0.0 {
        return Err(format!("{id}: bar must be a finite number at least 0"));
    }
    let before = raw.before.as_ref().map(|days| window(&id, "before", days)).transpose()?;
    let after = window(&id, "after", &raw.after)?;
    if before.is_some_and(|before| before.end > after.start) {
        return Err(format!("{id}: before must end by the day after starts"));
    }
    let review = Day::parse(&raw.review).ok_or_else(|| format!("{id}: review {} is not YYYY-MM-DD", raw.review))?;
    if review.unix() < after.end.unix() + REWORK_WINDOW_S {
        return Err(format!("{id}: review {} must fall at least 48 h after the after window ends ({}), so its last landings are judged for rework", review.text(), after.end.text()));
    }
    let owner = Issue::parse(&raw.owner).ok_or_else(|| format!("{id}: owner {} is not owner/repo#N", raw.owner))?;
    let scope = Scope { tier: raw.tier.filter(|tier| !tier.is_empty()), arm: raw.arm.filter(|arm| !arm.is_empty()) };
    if matches!(metric, Metric::IncidentRows(_)) && scope != Scope::default() {
        return Err(format!("{id}: an incident_rows_per_week entry reads one incident class, not a tier or arm"));
    }
    Ok(Experiment { id, change: collapse(&raw.change), landed: raw.landed, cited, metric, scope, direction, bar: raw.bar, min_n: raw.min_n, before, after, review, owner })
}

fn check(raw: RawCheck) -> Result<Check, String> {
    well_named(&raw.id)?;
    let id = raw.id;
    let text = collapse(&raw.text);
    if text.is_empty() {
        return Err(format!("{id}: box is empty"));
    }
    if raw.command.trim().is_empty() {
        return Err(format!("{id}: command is empty"));
    }
    let review = Day::parse(&raw.review).ok_or_else(|| format!("{id}: review {} is not YYYY-MM-DD", raw.review))?;
    let owner = Issue::parse(&raw.owner).ok_or_else(|| format!("{id}: owner {} is not owner/repo#N", raw.owner))?;
    Ok(Check { id, text, command: raw.command.trim().to_owned(), expect: raw.expect, review, owner })
}

fn collect<Raw, Entry>(raws: Vec<Raw>, build: impl Fn(Raw) -> Result<Entry, String>, id: impl Fn(&Entry) -> &str, problems: &mut Vec<String>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for raw in raws {
        match build(raw) {
            Ok(entry) if entries.iter().any(|seen| id(seen) == id(&entry)) => problems.push(format!("{}: id appears twice", id(&entry))),
            Ok(entry) => entries.push(entry),
            Err(why) => problems.push(why),
        }
    }
    entries
}

/// Parses and validates the registry, naming every invalid entry.
pub fn parse(text: &str) -> Result<Registry, Vec<String>> {
    let value = toml_json::parse(text).map_err(|why| vec![format!("{PATH}: {why}")])?;
    let raw: RawFile = serde_json::from_value(value).map_err(|why| vec![format!("{PATH}: {why}")])?;
    let mut problems = Vec::new();
    let experiments = collect(raw.experiment, entry, |entry| &entry.id, &mut problems);
    let checks = collect(raw.check, check, |entry| &entry.id, &mut problems);
    if let Some(shared) = checks.iter().find(|check| experiments.iter().any(|experiment| experiment.id == check.id)) {
        problems.push(format!("{}: id appears in both [[experiment]] and [[check]]", shared.id));
    }
    if problems.is_empty() { Ok(Registry { experiments, checks }) } else { Err(problems) }
}
