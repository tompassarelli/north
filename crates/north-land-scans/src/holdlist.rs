use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, Item, Table};

use crate::{Range, gate_shadow, git};

const SELF: &str = "holdlist-guards-resolve";
const HOLDLIST: &str = "harness/holdlist.toml";
const REGISTRY: &str = "harness/gates.toml";
const CONCERNS: &str = "concerns.toml";
const LAND_CONFIG: &str = ".safe-push";
const EVALS: &str = "harness/evals/";
const TRAILER: &str = "Holdlist";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Test,
    Concern,
    Eval,
    LandCheck,
}

impl Kind {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "test" => Self::Test,
            "concern" => Self::Concern,
            "eval" => Self::Eval,
            "land-check" => Self::LandCheck,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Concern => "concern",
            Self::Eval => "eval",
            Self::LandCheck => "land-check",
        }
    }

    fn sought(self, name: &str) -> String {
        match self {
            Self::Test => format!("no `fn {name}(` is in the lane's Rust files"),
            Self::Concern => format!("{CONCERNS} has no [[concern]] of that name"),
            Self::Eval => format!("no file named {name} is under {EVALS}"),
            Self::LandCheck => format!("no land.check or land.after line of {LAND_CONFIG} names it"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Guard {
    pub hold: String,
    pub kind: Kind,
    pub name: String,
    pub live: bool,
}

impl Guard {
    fn named(&self) -> String {
        format!("{}: {} `{}`", self.hold, self.kind.label(), self.name)
    }

    fn same(&self, other: &Self) -> bool {
        self.hold == other.hold && self.kind == other.kind && self.name == other.name
    }
}

fn guard_of(hold: &str, table: &Table) -> Result<Guard, String> {
    let field = |key: &str| table.get(key).and_then(Item::as_str).map(str::trim).filter(|text| !text.is_empty());
    let name = field("guard").ok_or_else(|| format!("{hold}: a guard has no `guard` name"))?;
    let kind = field("guard_kind").and_then(Kind::parse).ok_or_else(|| format!("{hold}: `guard_kind` of `{name}` is not test, concern, eval or land-check"))?;
    let live = match field("state") {
        Some("live") => true,
        Some("missing") => false,
        _ => return Err(format!("{hold}: `state` of `{name}` is not live or missing")),
    };
    if !live && field("accepted_risk").is_none() {
        return Err(format!("{hold}: `{name}` is missing with no `accepted_risk` naming a date and a reason"));
    }
    Ok(Guard { hold: hold.to_owned(), kind, name: name.to_owned(), live })
}

pub fn guards(text: &str) -> Result<Vec<Guard>, String> {
    let document = text.parse::<DocumentMut>().map_err(|error| format!("{HOLDLIST} does not parse: {error}"))?;
    let Some(holds) = document.get("hold").and_then(Item::as_array_of_tables) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for hold in holds {
        let id = hold.get("id").and_then(Item::as_str).ok_or_else(|| format!("a [[hold]] of {HOLDLIST} has no `id`"))?;
        found.push(guard_of(id, hold)?);
        for also in hold.get("also").and_then(Item::as_array_of_tables).into_iter().flatten() {
            found.push(guard_of(id, also)?);
        }
    }
    Ok(found)
}

fn file_at(directory: &Path, revision: &str, path: &str) -> Option<String> {
    git::text(directory, &["show", &format!("{revision}:{path}")])
}

fn tests_in(directory: &Path, head: &str, names: &[&str]) -> BTreeSet<String> {
    let names: Vec<&&str> = names.iter().filter(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')).collect();
    if names.is_empty() {
        return BTreeSet::new();
    }
    let pattern = format!("fn ({})\\(", names.iter().map(|name| **name).collect::<Vec<_>>().join("|"));
    git::text(directory, &["grep", "-h", "-o", "-E", &pattern, head, "--", "*.rs"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("fn ")?.strip_suffix('('))
        .map(str::to_owned)
        .collect()
}

fn concerns_in(directory: &Path, head: &str) -> BTreeSet<String> {
    let Some(document) = file_at(directory, head, CONCERNS).and_then(|text| text.parse::<DocumentMut>().ok()) else {
        return BTreeSet::new();
    };
    document.get("concern").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|concern| concern.get("name").and_then(Item::as_str)).map(str::to_owned).collect()
}

fn evals_in(directory: &Path, head: &str) -> BTreeSet<String> {
    git::text(directory, &["ls-tree", "-r", "--name-only", head, "--", EVALS])
        .unwrap_or_default()
        .lines()
        .filter_map(|path| path.rsplit('/').next())
        .map(|file| file.split_once('.').map_or(file, |(stem, _)| stem).to_owned())
        .collect()
}

fn land_check_in(directory: &Path, head: &str, name: &str) -> bool {
    let named = |line: &str| line.split(|character: char| !(character.is_ascii_alphanumeric() || character == '-' || character == '_')).any(|word| word == name);
    file_at(directory, head, LAND_CONFIG).is_some_and(|text| text.lines().map(str::trim_start).filter(|line| line.starts_with("check =") || line.starts_with("after =")).any(named))
}

pub fn unresolved(directory: &Path, head: &str, holds: &[Guard]) -> Vec<String> {
    let live: Vec<&Guard> = holds.iter().filter(|guard| guard.live).collect();
    let of = |kind: Kind| live.iter().filter(move |guard| guard.kind == kind);
    let tests = tests_in(directory, head, &of(Kind::Test).map(|guard| guard.name.as_str()).collect::<Vec<_>>());
    let concerns = if of(Kind::Concern).next().is_some() { concerns_in(directory, head) } else { BTreeSet::new() };
    let evals = if of(Kind::Eval).next().is_some() { evals_in(directory, head) } else { BTreeSet::new() };
    live.iter()
        .filter(|guard| match guard.kind {
            Kind::Test => !tests.contains(&guard.name),
            Kind::Concern => !concerns.contains(&guard.name),
            Kind::Eval => !evals.contains(&guard.name),
            Kind::LandCheck => !land_check_in(directory, head, &guard.name),
        })
        .map(|guard| format!("{} is live in {HOLDLIST} but {}", guard.named(), guard.kind.sought(&guard.name)))
        .collect()
}

pub fn weakened(before: &[Guard], after: &[Guard]) -> Vec<String> {
    let kept: BTreeSet<&str> = after.iter().map(|guard| guard.hold.as_str()).collect();
    let mut found: Vec<String> = before.iter().map(|guard| guard.hold.as_str()).filter(|hold| !kept.contains(hold)).collect::<BTreeSet<_>>().into_iter().map(|hold| format!("{hold}: the hold is removed from {HOLDLIST}")).collect();
    found.extend(
        before
            .iter()
            .filter(|guard| guard.live && kept.contains(guard.hold.as_str()))
            .filter(|guard| !after.iter().any(|now| now.live && now.same(guard)))
            .map(|guard| format!("{} is no longer a live guard in {HOLDLIST}", guard.named())),
    );
    found
}

fn excused(directory: &Path, fork: &str, head: &str) -> bool {
    let trailers = git::text(directory, &["log", "--no-merges", &format!("--format=%(trailers:key={TRAILER},valueonly)"), &format!("{fork}..{head}")]).unwrap_or_default();
    trailers.lines().any(|line| !line.trim().is_empty())
}

pub fn check(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let before = file_at(directory, &fork, HOLDLIST).and_then(|text| guards(&text).ok()).unwrap_or_default();
    let after = match file_at(directory, head, HOLDLIST) {
        Some(text) => guards(&text)?,
        None => Vec::new(),
    };
    let mut refusals = unresolved(directory, head, &after);
    let given_up = weakened(&before, &after);
    if !given_up.is_empty() && !excused(directory, &fork, head) {
        refusals.extend(given_up.into_iter().map(|line| format!("{line}, and no commit of the lane says why in a `{TRAILER}: REASON` trailer")));
    }
    match refusals.is_empty() {
        true => Ok(()),
        false => Err(refusals.join("\n")),
    }
}

fn enforcing(directory: &Path, revisions: [&str; 2]) -> bool {
    revisions.iter().any(|revision| file_at(directory, revision, REGISTRY).is_some_and(|text| gate_shadow::gates(&text).get(SELF).is_some_and(|gate| gate.enforcing)))
}

fn record(state: &Path, head: &str, refusal: Option<&str>) {
    let directory = state.join("north/gates");
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": SELF, "at": at, "head": head, "would_refuse": refusal.is_some(), "holds": refusal.map(|text| text.lines().collect::<Vec<_>>()).unwrap_or_default()});
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(directory.join(format!("{SELF}.jsonl"))) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn scan(directory: &Path, range: &str, state: &Path, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let outcome = check(directory, base, head);
    let enforce = enforcing(directory, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        record(state, head, outcome.as_ref().err().map(String::as_str));
    }
    let Err(refusal) = outcome else {
        return 0;
    };
    let verb = if enforce { "refused" } else { "would refuse (shadow)" };
    let _ = writeln!(out, "{SELF}: {verb}: the lane leaves a hold of {HOLDLIST} without its guard:");
    for line in refusal.lines() {
        let _ = writeln!(out, "  {line}");
    }
    u8::from(enforce)
}

#[cfg(test)]
#[path = "holdlist_tests.rs"]
mod tests;
