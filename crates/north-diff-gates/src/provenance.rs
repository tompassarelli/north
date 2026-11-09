use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use crate::platform::{git as platform_git, github};
use std::sync::LazyLock;

use crate::foundation::clock;

const FORMS: &str = "Provenance: <claim with the bar>; origin <tom|experiment|prior-art|incident|none>; pointer <quote with date | run file plus rerun command | link | issue>; rationale <repo path#anchor | not recorded>; revisit <YYYY-MM-DD | closed REPO#N | event text>, ending ; soft for a bar that does not gate (origin none only then)";
const UNIT: &str = r"(seconds|minutes|instructions|allocations|launches|frames|tokens|bytes|lines|units|sec|min|fps|MiB|GiB|KiB|MB|KB|GB|ns|µs|us|ms|s|h|%)";
const NUMBER: &str = r"(\d[\d,]*(?:\.\d+)?)";
static BAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?:(?i:(at least))\s+{NUMBER}\s*[×x]\s+(?i:fewer)\s+{UNIT})|(?:(?i:(at or under|at most|no more than|less than|faster than|under|below|within))\s+|(<=|≤|<)\s*){NUMBER}\s*{UNIT}")).unwrap()
});

macro_rules! re {
    ($pattern:literal) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
        &*RE
    }};
}

fn origin(s: &str) -> bool {
    matches!(s, "tom" | "experiment" | "prior-art" | "incident")
}
fn quantity(number: &str, unit: &str) -> (String, String) {
    let number: String = number
        .chars()
        .filter(|c| *c != ',')
        .map(|c| {
            if c.is_ascii() {
                return c;
            }
            let mut first = c as u32;
            while first > 0
                && char::from_u32(first - 1).is_some_and(|p| re!(r"^\d$").is_match(&p.to_string()))
            {
                first -= 1;
            }
            char::from_digit((c as u32 - first) % 10, 10).unwrap()
        })
        .collect();
    let number = number.parse::<f64>().unwrap_or(f64::NAN);
    let raw = format!("{number:.5e}");
    let mut number = if !number.is_finite() {
        number.to_string()
    } else {
        let (mantissa, exp) = raw.split_once('e').unwrap();
        let exp: i32 = exp.parse().unwrap();
        if !(-4..6).contains(&exp) {
            format!(
                "{}e{exp:+03}",
                mantissa.trim_end_matches('0').trim_end_matches('.')
            )
        } else {
            format!(
                "{:.*}",
                (5 - exp).max(0) as usize,
                raw.parse::<f64>().unwrap()
            )
        }
    };
    if number.contains('.') && !number.contains('e') {
        number = number.trim_end_matches('0').trim_end_matches('.').into();
    }
    (
        number,
        match unit {
            "sec" | "seconds" => "s",
            "minutes" => "min",
            "µs" => "us",
            _ => unit,
        }
        .into(),
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Bar {
    comparator: String,
    number: String,
    unit: String,
    text: String,
}
fn word(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}
pub fn bars(text: &str) -> Vec<Bar> {
    BAR.captures_iter(text)
        .filter_map(|m| {
            let whole = m.get(0).unwrap();
            let symbolic = m.get(5).is_some();
            if !symbolic && text[..whole.start()].chars().next_back().is_some_and(word) {
                return None;
            }
            let (cmp, number, unit) = if m.get(1).is_some() {
                (&m[1], &m[2], &m[3])
            } else {
                (m.get(4).or(m.get(5)).unwrap().as_str(), &m[6], &m[7])
            };
            let end = m.get(if m.get(1).is_some() { 3 } else { 7 }).unwrap().end();
            if text[end..].chars().next().is_some_and(word) {
                return None;
            }
            let cmp = cmp.to_lowercase();
            let comparator = match cmp.as_str() {
                "<=" | "≤" | "at or under" | "no more than" => "at most",
                "<" | "less than" | "below" => "under",
                "at least" => "at least x fewer",
                _ => &cmp,
            }
            .into();
            let (number, unit) = quantity(number, unit);
            Some(Bar {
                comparator,
                number,
                unit,
                text: whole.as_str().into(),
            })
        })
        .collect()
}

fn fields(value: &str) -> (&str, BTreeMap<&str, &str>, bool) {
    let parts: Vec<_> = value.split(';').map(str::trim).collect();
    let named = parts
        .iter()
        .skip(1)
        .filter_map(|p| {
            p.find(char::is_whitespace)
                .map(|i| (&p[..i], p[i..].trim_start()))
        })
        .collect();
    (parts[0], named, parts.last() == Some(&"soft"))
}
fn records<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let pattern = match key {
        "Provenance:" => re!(r"(?m)^Provenance:\s*(.+)$"),
        "Target-debt:" => re!(r"(?m)^Target-debt:\s*(.+)$"),
        "Target-paid:" => re!(r"(?m)^Target-paid:\s*(.+)$"),
        _ => unreachable!(),
    };
    pattern
        .captures_iter(text)
        .map(|m| m.get(1).unwrap().as_str())
        .collect()
}
fn valid_record(value: &str) -> bool {
    let (claim, named, soft) = fields(value);
    !claim.is_empty()
        && ["pointer", "rationale", "revisit"]
            .iter()
            .all(|k| named.get(k).is_some_and(|v| !v.is_empty()))
        && named
            .get("rationale")
            .is_some_and(|r| *r == "not recorded" || re!(r"\A[^\s#]+#[^\s#]+\z").is_match(r))
        && (origin(named.get("origin").copied().unwrap_or(""))
            || soft && named.get("origin") == Some(&"none"))
}
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    platform_git::stdout(repo, args, |output| String::from_utf8_lossy(&output.stderr).into_owned())
        .map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
}
fn gating(path: &str, line: &str) -> bool {
    re!(r"(^|/)PLAN[^/]*\.md$").is_match(path) && line.trim_start().starts_with('|')
        || line.contains("Done when")
        || re!(r"^\s*[-*] \[ \]").is_match(line)
}
fn new_bars(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, Bar, String)>, String> {
    let base = git(repo, &["merge-base", base, head])
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| base.into());
    let mut added = Vec::new();
    let mut removed = BTreeSet::new();
    let mut path = "";
    let mut header = false;
    let diff = git(
        repo,
        &[
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            &base,
            head,
        ],
    )?;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if header && line.starts_with("--- ") {
            if let Some(p) = line.strip_prefix("--- a/") {
                path = p;
            }
        } else if header && line.starts_with("+++ ") {
            if let Some(p) = line.strip_prefix("+++ b/") {
                path = p;
            }
            header = false;
        } else if !header && line.starts_with('-') {
            for bar in bars(&line[1..]) {
                removed.insert((path.to_owned(), bar.comparator, bar.number, bar.unit));
            }
        } else if !header && line.starts_with('+') && gating(path, &line[1..]) {
            for bar in bars(&line[1..]) {
                added.push((path.to_owned(), bar, line[1..].trim().to_owned()));
            }
        }
    }
    let mut covered = BTreeSet::new();
    let log = git(repo, &["log", "--format=%B", &format!("{base}..{head}")])?;
    static QUANTITIES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r"{NUMBER}\s*(?:[×x]\s+(?i:fewer)\s+)?{UNIT}")).unwrap()
    });
    let quantities = &*QUANTITIES;
    for value in records(&log, "Provenance:") {
        let (claim, named, soft) = fields(value);
        if named.contains_key("rationale")
            && (origin(named.get("origin").copied().unwrap_or(""))
                || soft && named.get("origin") == Some(&"none"))
        {
            for m in quantities.captures_iter(claim) {
                let m0 = m.get(0).unwrap();
                if claim[..m0.start()]
                    .chars()
                    .next_back()
                    .is_some_and(|c| word(c) || c == '.')
                {
                    continue;
                }
                let end = m.get(2).unwrap().end();
                if claim[end..].chars().next().is_some_and(word) {
                    continue;
                }
                covered.insert(quantity(&m[1], &m[2]));
            }
        }
    }
    let mut seen = BTreeSet::new();
    Ok(added
        .into_iter()
        .filter(|(p, b, l)| {
            !removed.contains(&(
                p.clone(),
                b.comparator.clone(),
                b.number.clone(),
                b.unit.clone(),
            )) && !covered.contains(&(b.number.clone(), b.unit.clone()))
                && seen.insert((p.clone(), b.clone(), l.clone()))
        })
        .collect())
}
fn rule_text(line: &str) -> String {
    re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)")
        .replace(line, "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn policy_name(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|s| s.to_str()),
        Some("AGENTS.md" | "SKILL.md" | "catalog-config.json")
    )
}
fn policy_line(path: &str, line: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    matches!(name, "AGENTS.md" | "SKILL.md")
        && re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)").is_match(line)
        || name == "catalog-config.json" && re!(r#""kind"\s*:\s*"hook""#).is_match(line)
}
fn rule_items(path: &str, text: &str) -> Vec<(usize, usize, String)> {
    let mut items: Vec<(usize, usize, String)> = Vec::new();
    let mut fenced = false;
    let mut routers = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if Path::new(path)
            .file_name()
            .is_some_and(|n| n == "AGENTS.md")
            && let Some(h) = re!(r"^(#{1,6})\s+(.+)").captures(line) {
                if routers > 0 && h[1].len() <= routers {
                    routers = 0;
                }
                if h[2].trim() == "Routers" {
                    routers = h[1].len();
                }
            }
        if routers > 0 {
            continue;
        }
        if re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)").is_match(line) || policy_line(path, line) {
            items.push((number, number, line.trim().into()));
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty()
            && let Some(last) = items.last_mut()
                && last.1 == number - 1 {
                    last.1 = number;
                    last.2.push(' ');
                    last.2.push_str(line.trim());
                }
    }
    items
}
fn without_paths(text: &str) -> String {
    re!(r"`[^\s`]*(?:/[^\s`]*|\.[A-Za-z][^\s`]*)`|\b[^\s`()]*[/][^\s`()]+")
        .replace_all(text, "")
        .into_owned()
}
fn words(text: &str) -> BTreeSet<String> {
    re!(r"\w+")
        .find_iter(
            &re!(r"`[^`]*`")
                .replace_all(&without_paths(text), "")
                .to_lowercase(),
        )
        .map(|m| m.as_str().into())
        .collect()
}
fn new_rules(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, String, String)>, String> {
    let base = git(repo, &["merge-base", base, head])?.trim().to_owned();
    let log = git(repo, &["log", "--format=%B", &format!("{base}..{head}")])?;
    let claims: Vec<_> = records(&log, "Provenance:")
        .into_iter()
        .filter(|v| valid_record(v))
        .map(|v| rule_text(fields(v).0))
        .collect();
    let names = git(
        repo,
        &["diff", "--name-status", "-z", "--find-renames", &base, head],
    )?;
    let mut names = names.split_terminator('\0');
    let mut added = Vec::new();
    let mut removed = Vec::new();
    while let Some(status) = names.next() {
        let name = names.next().ok_or("missing diff path")?;
        let (mut old, mut path) = if status.starts_with(['R', 'C']) {
            (name, names.next().ok_or("missing renamed path")?)
        } else {
            (name, name)
        };
        if status == "A" {
            old = "";
            if path.starts_with("harness/hooks/")
                && !["/lib/", "/fixtures/", ".test."]
                    .iter()
                    .any(|s| path.contains(s))
            {
                added.push((path.to_owned(), "new hook".into(), path.to_owned()));
            }
        } else if status == "D" {
            path = "";
        }
        if !policy_name(old) && !policy_name(path) {
            continue;
        }
        let mut args = vec![
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            &base,
            head,
            "--",
        ];
        if !old.is_empty() {
            args.push(old);
        }
        if !path.is_empty() && path != old {
            args.push(path);
        }
        let mut deleted = BTreeSet::new();
        let mut inserted = BTreeSet::new();
        let mut hunk = None;
        for line in git(repo, &args)?.lines() {
            if let Some(m) = re!(r"^@@ -(\d+)(?:,\d+)? \+(\d+)").captures(line) {
                hunk = Some((
                    m[1].parse::<usize>().unwrap(),
                    m[2].parse::<usize>().unwrap(),
                ));
            } else if let Some((o, n)) = hunk.as_mut() {
                if line.starts_with('-') {
                    deleted.insert(*o);
                    *o += 1;
                } else if line.starts_with('+') {
                    inserted.insert(*n);
                    *n += 1;
                }
            }
        }
        if !old.is_empty() && !deleted.is_empty() && policy_name(old) {
            for (start, end, text) in
                rule_items(old, &git(repo, &["show", &format!("{base}:{old}")])?)
            {
                if (start..=end).any(|n| deleted.contains(&n)) {
                    removed.push(text);
                }
            }
        }
        if !path.is_empty() && !inserted.is_empty() && policy_name(path) {
            for (start, end, text) in
                rule_items(path, &git(repo, &["show", &format!("{head}:{path}")])?)
            {
                if (start..=end).any(|n| inserted.contains(&n)) && policy_line(path, &text) {
                    added.push((path.into(), "new policy line".into(), text));
                }
            }
        }
    }
    let corpus: BTreeSet<_> = removed.iter().flat_map(|t| words(t)).collect();
    let protected = re!(
        r"(?i)\d+(?:\.\d+)?[a-z]*|[<>≤≥]=?|\b(?:one|two|three|four|five|six|seven|eight|nine|ten|few|hundred|thousand|million|billion|never|always|only|must|may|should|not|no|unless|require|refuse|default|under|over|above|below|within|least|most|less|more|fewer|before|after)\b"
    );
    let mut fresh = Vec::new();
    for (path, kind, text) in added {
        let current = rule_text(&text);
        if claims
            .iter()
            .any(|c| c.contains(&path) || c.contains(&current))
        {
            continue;
        }
        if kind == "new hook" {
            fresh.push((path, kind, text));
            continue;
        }
        let found = removed.iter().position(|previous| {
            if current == rule_text(previous) {
                return true;
            }
            let before = without_paths(previous);
            let after = without_paths(&text);
            let b = before.to_lowercase();
            let a = after.to_lowercase();
            if protected
                .find_iter(&b)
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                != protected
                    .find_iter(&a)
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
            {
                return false;
            }
            let old_words = words(&before);
            let new_words = words(&after);
            if (old_words.contains("default") || new_words.contains("default")) && before != after {
                return false;
            }
            old_words == new_words
                || !old_words.is_empty()
                    && !new_words.is_empty()
                    && old_words.intersection(&new_words).count() as f64
                        / old_words.len().min(new_words.len()) as f64
                        >= 0.85
                    && new_words.difference(&corpus).count() <= 1
        });
        if let Some(i) = found {
            removed.remove(i);
        } else {
            fresh.push((path, kind, text));
        }
    }
    let mut seen = BTreeSet::new();
    Ok(fresh
        .into_iter()
        .filter(|r| seen.insert(r.clone()))
        .collect())
}
fn check(repo: &Path, base: &str, head: &str) -> Result<u8, String> {
    let body = git(repo, &["log", "-1", "--format=%B", head])?;
    for m in re!(
        r"(?m)^This reverts commit ([0-9a-f]{7,40})\.?(?:\s*)$|^Sheriff-Reverts: ([0-9a-f]{40})$"
    )
    .captures_iter(&body)
    {
        if git(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", m.get(1).or(m.get(2)).unwrap().as_str()),
            ],
        )
        .is_ok()
        {
            return Ok(0);
        }
    }
    let missing = new_bars(repo, base, head)?;
    let rules = new_rules(repo, base, head)?;
    for (path, bar, line) in &missing {
        println!("{path}\t{}\t{line}", bar.text);
    }
    for (path, kind, line) in &rules {
        println!("{path}\t{kind}\t{line}");
    }
    if !missing.is_empty() {
        eprintln!(
            "provenance: {} new gating bars have no Provenance record; add {FORMS}",
            missing.len()
        );
    }
    if !rules.is_empty() {
        eprintln!(
            "provenance: {} new hooks or policy lines have no valid Provenance record; name the rule or its source path in the claim; add {FORMS}",
            rules.len()
        );
    }
    Ok(u8::from(!missing.is_empty() || !rules.is_empty()))
}
fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}
fn repos(args: &[String]) -> Vec<PathBuf> {
    if !args.is_empty() {
        return args.iter().map(PathBuf::from).collect();
    }
    let mut result: Vec<_> = std::fs::read_dir(home().join("code"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path().join("main"))
        .filter(|p| p.exists())
        .collect();
    result.sort();
    result
}
fn repo_name(repo: &Path) -> String {
    let path = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let path = if path.file_name().is_some_and(|n| n == "main") {
        path.parent().unwrap_or(&path)
    } else {
        &path
    };
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn issue_due(value: &str) -> Result<Option<bool>, String> {
    let Some(m) = re!(r"\Aclosed\s+([\w.-]+)#(\d+)\z").captures(value) else {
        return Ok(None);
    };
    let out = github::run(&[
        "issue",
        "view",
        &m[2],
        "-R",
        &format!("tompassarelli/{}", &m[1]),
        "--json",
        "state",
    ])
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(Some(false));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let state = value.get("state").ok_or("gh response has no state")?;
    Ok(Some(state == "CLOSED"))
}
fn due(value: &str, event: bool) -> Result<bool, String> {
    Ok(issue_due(value)?.unwrap_or_else(|| clock::parse_python_date(value).map_or(event, |d| d <= clock::local_today())))
}
fn debt(paths: &[String], only_due: bool) -> Result<(), String> {
    for repo in repos(paths) {
        if !repo.is_dir() {
            return Err(format!("repository does not exist: {}", repo.display()));
        }
        let output = platform_git::run(&repo, &["log", "origin/main", "-z", "--format=%H%x00%B"])
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            continue;
        }
        let log = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<_> = log.split('\0').collect();
        let commits: Vec<_> = parts.chunks_exact(2).map(|v| (v[0], v[1])).collect();
        let paid: Vec<_> = commits
            .iter()
            .flat_map(|(_, body)| records(body, "Target-paid:"))
            .filter_map(|v| {
                re!(r"^[0-9a-f]{7,40}")
                    .find(v)
                    .map(|m| m.as_str().to_owned())
            })
            .collect();
        for (sha, body) in commits {
            if paid.iter().any(|p| sha.starts_with(p)) {
                continue;
            }
            for value in records(body, "Target-debt:") {
                let (bar, named, _) = fields(value);
                let revisit = named.get("revisit").copied().unwrap_or("");
                if !only_due || due(revisit, true)? {
                    println!(
                        "{}\t{}\t{bar}\t{}\t{}\t{revisit}",
                        repo_name(&repo),
                        &sha[..sha.len().min(12)],
                        named.get("measured").unwrap_or(&""),
                        named.get("owner").unwrap_or(&"")
                    );
                }
            }
        }
    }
    Ok(())
}
const HELP: &str = r#"usage: north check provenance [-h] {check,debt} ...

provenance: refuse new steering without provenance, and list Target-debt.

positional arguments:
  {check,debt}
    check             list new gating bars, hooks and policy lines without a
                      Provenance record
    debt              list Target-debt trailers on origin/main not named by a
                      Target-paid

options:
  -h, --help          show this help message and exit
"#;
const HELP_CHECK: &str = r#"usage: north check provenance check [-h] base head

positional arguments:
  base
  head

options:
  -h, --help  show this help message and exit
"#;
const HELP_DEBT: &str = r#"usage: north check provenance debt [-h] [--due] [--repo REPO]

options:
  -h, --help   show this help message and exit
  --due        only rows whose revisit date passed or issue closed, and every
               event text
  --repo REPO  repository path (default ~/code/*/main)
"#;
enum Invocation {
    Check { base: String, head: String },
    Debt { paths: Vec<String>, only_due: bool },
}

#[derive(Debug)]
enum Exit {
    Help(&'static str),
    Usage { command: String, message: String },
}

impl Exit {
    fn usage(command: &str, message: impl Into<String>) -> Self {
        Exit::Usage { command: command.to_owned(), message: message.into() }
    }

    fn code(&self) -> u8 {
        match self {
            Exit::Help(_) => 0,
            Exit::Usage { .. } => 2,
        }
    }

    fn stdout(&self) -> &str {
        match self {
            Exit::Help(text) => text,
            Exit::Usage { .. } => "",
        }
    }

    fn stderr(&self) -> String {
        let Exit::Usage { command, message } = self else {
            return String::new();
        };
        let help = match command.as_str() {
            "check" => HELP_CHECK,
            "debt" => HELP_DEBT,
            _ => HELP,
        };
        let name = if command.is_empty() { "provenance".to_owned() } else { format!("provenance {command}") };
        format!("{}\n{name}: error: {message}\n", help.lines().next().unwrap())
    }
}

fn is_help(key: &str) -> bool {
    key == "-h" || (key.len() > 2 && "--help".starts_with(key))
}

fn parse(args: &[String]) -> Result<Invocation, Exit> {
    let mut start = 0;
    while start < args.len() && args[start].starts_with('-') && args[start] != "--" {
        let (key, value) = args[start]
            .split_once('=')
            .map_or((args[start].as_str(), None), |(k, v)| (k, Some(v)));
        if is_help(key) {
            if let Some(value) = value {
                return Err(Exit::usage("", format!("argument -h/--help: ignored explicit argument '{value}'")));
            }
            return Err(Exit::Help(HELP));
        }
        start += 1;
    }
    let leading = &args[..start];
    let args = &args[start..];
    let Some(command) = args.first() else {
        return Err(Exit::usage("", "the following arguments are required: command"));
    };
    if command == "check" {
        let mut positional = Vec::new();
        let mut unknown = Vec::new();
        let mut options = true;
        for arg in &args[1..] {
            let (key, value) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
            if options && is_help(key) {
                if let Some(value) = value {
                    return Err(Exit::usage("check", format!("argument -h/--help: ignored explicit argument '{value}'")));
                }
                return Err(Exit::Help(HELP_CHECK));
            } else if options && arg == "--" {
                options = false;
            } else if options && arg.starts_with('-') && !re!(r"\A-\d+$|\A-\d*\.\d+$").is_match(arg)
            {
                unknown.push(arg.as_str());
            } else if positional.len() < 2 {
                positional.push(arg);
            } else {
                unknown.push(arg.as_str());
            }
        }
        if positional.len() < 2 {
            return Err(Exit::usage(
                "check",
                if positional.is_empty() {
                    "the following arguments are required: base, head"
                } else {
                    "the following arguments are required: head"
                },
            ));
        }
        unknown.splice(0..0, leading.iter().map(String::as_str));
        if !unknown.is_empty() {
            return Err(Exit::usage("", format!("unrecognized arguments: {}", unknown.join(" "))));
        }
        return Ok(Invocation::Check { base: positional[0].clone(), head: positional[1].clone() });
    }
    if command != "debt" {
        return Err(Exit::usage(
            "",
            format!("argument command: invalid choice: '{command}' (choose from 'check', 'debt')"),
        ));
    }
    let mut paths = Vec::new();
    let mut only_due = false;
    let mut unknown: Vec<_> = leading.iter().map(String::as_str).collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--" {
            unknown.extend(args[i..].iter().map(String::as_str));
            break;
        }
        let (key, inline) = args[i]
            .split_once('=')
            .map_or((args[i].as_str(), None), |(k, v)| (k, Some(v)));
        if is_help(key) {
            if let Some(value) = inline {
                return Err(Exit::usage(command, format!("argument -h/--help: ignored explicit argument '{value}'")));
            }
            return Err(Exit::Help(HELP_DEBT));
        }
        let options: &[&str] = &["--due", "--repo"];
        let candidates: Vec<_> = options
            .iter()
            .copied()
            .filter(|opt| key.starts_with("--") && opt.starts_with(key))
            .collect();
        if candidates.len() > 1 {
            return Err(Exit::usage(command, format!("ambiguous option: {key} could match {}", candidates.join(", "))));
        }
        let key = candidates.first().copied().unwrap_or(key);
        match key {
            "--due" => {
                if let Some(value) = inline {
                    return Err(Exit::usage(command, format!("argument --due: ignored explicit argument '{value}'")));
                }
                only_due = true;
            }
            "--repo" => {
                let value = if let Some(v) = inline {
                    v
                } else {
                    i += 1;
                    if i >= args.len()
                        || (args[i].starts_with('-')
                            && !re!(r"\A-\d+$|\A-\d*\.\d+$").is_match(&args[i]))
                    {
                        return Err(Exit::usage(command, format!("argument {key}: expected one argument")));
                    }
                    &args[i]
                };
                paths.push(value.to_owned());
            }
            _ => {
                unknown.push(args[i].as_str());
            }
        }
        i += 1;
    }
    if !unknown.is_empty() {
        return Err(Exit::usage("", format!("unrecognized arguments: {}", unknown.join(" "))));
    }
    Ok(Invocation::Debt { paths, only_due })
}

pub fn run(args: &[String]) -> u8 {
    let invocation = match parse(args) {
        Ok(invocation) => invocation,
        Err(exit) => {
            print!("{}", exit.stdout());
            eprint!("{}", exit.stderr());
            return exit.code();
        }
    };
    let result = match invocation {
        Invocation::Check { base, head } => {
            return match check(Path::new("."), &base, &head) {
                Ok(c) => c,
                Err(e) => {
                    eprint!("{e}");
                    1
                }
            };
        }
        Invocation::Debt { paths, only_due } => debt(&paths, only_due),
    };
    if let Err(error) = result {
        eprintln!("provenance: {error}");
        return 1;
    }
    0
}
