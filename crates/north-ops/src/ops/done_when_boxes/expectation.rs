use std::sync::LazyLock;

use regex::Regex;

static THRESHOLD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(at most|at least|no more than|no less than|fewer than|more than|less than|greater than|under|over|below|above|within|exceeds?|ratio|median|percent)\b|[<>≤≥]|\d%").expect("threshold pattern")
});
static EMPTY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(prints|outputs|shows|returns|lists|finds) +(nothing|no output|no lines?|no match(es)?|no results?)\b|\b(has|gives|yields) +no (output|match(es)?|results?)\b|\bmatches nothing\b|\boutput is empty\b|\bis empty\b").expect("empty pattern")
});
static SHOWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(prints|outputs|shows|reports|lists) +").expect("shows pattern"));
static EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(exits|exit|returns) +(code +|status +)?(\d+)\b").expect("exit pattern"));
static PASSES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(passes|pass|succeeds|succeed|is green|are green)\b").expect("passes pattern"));
static CLAUSE_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.,;:]( |$)|\.$| and | or | with | on | for | in ").expect("clause pattern"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"`[^`]*`|"[^"]*""#).expect("quoted pattern"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expectation {
    Exit(i32),
    Empty,
    Contains(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expected {
    All(Vec<Expectation>),
    Judge(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Pass,
    Fail,
    Judge,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Judge => "judge",
        }
    }
}

fn shown(verb: &str, after: &str) -> Result<String, String> {
    let mut characters = after.chars();
    if let Some(quote @ ('`' | '"')) = characters.next() {
        return match characters.as_str().split_once(quote) {
            Some((inside, _)) if !inside.trim().is_empty() => Ok(inside.to_owned()),
            _ => Err(format!("nothing named after `{verb}`")),
        };
    }
    match CLAUSE_END.find(after).map_or(after, |end| &after[..end.start()]).trim() {
        "" => Err(format!("nothing named after `{verb}`")),
        described => Err(format!("an unquoted description after `{verb}` ({described}), which only a quoted phrase turns into text to find")),
    }
}

fn unquoted(text: &str) -> String {
    QUOTED.replace_all(text, |quoted: &regex::Captures| format!("{0}{1}{0}", &quoted[0][..1], " ".repeat(quoted[0].len() - 2))).into_owned()
}

pub fn expected(text: &str) -> Expected {
    let text = text.trim();
    if text.is_empty() {
        return Expected::Judge("no stated expectation".into());
    }
    let mut masked = unquoted(text);
    if let Some(found) = THRESHOLD.find(&masked) {
        return Expected::Judge(format!("a threshold the program cannot read ({})", found.as_str()));
    }
    let mut all = Vec::new();
    let mut rest = text.to_owned();
    if let Some(range) = EMPTY.find(&masked).map(|found| found.range()) {
        all.push(Expectation::Empty);
        let blank = " ".repeat(range.len());
        rest.replace_range(range.clone(), &blank);
        masked.replace_range(range, &blank);
    }
    for found in SHOWS.find_iter(&masked) {
        match shown(found.as_str().trim(), &rest[found.end()..]) {
            Ok(shown) => all.push(Expectation::Contains(shown)),
            Err(reason) => return Expected::Judge(reason),
        }
    }
    if let Some(code) = EXIT.captures(&masked).and_then(|found| found[3].parse().ok()) {
        all.push(Expectation::Exit(code));
    } else if PASSES.is_match(&masked) {
        all.push(Expectation::Exit(0));
    }
    if all.is_empty() { Expected::Judge("no expectation the program can read".into()) } else { Expected::All(all) }
}

pub fn outcome(expected: &Expected, exit: i32, stdout: &str, output: &str) -> Outcome {
    let holds = |expectation: &Expectation| match expectation {
        Expectation::Exit(code) => exit == *code,
        Expectation::Empty => stdout.trim().is_empty() && exit < 2,
        Expectation::Contains(shown) => output.contains(shown.as_str()),
    };
    match expected {
        Expected::Judge(_) => Outcome::Judge,
        Expected::All(all) if all.iter().all(holds) => Outcome::Pass,
        Expected::All(_) => Outcome::Fail,
    }
}
