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

fn shown(after: &str) -> Option<String> {
    let mut characters = after.chars();
    let quoted = match characters.next()? {
        quote @ ('`' | '"') => characters.as_str().split_once(quote).map(|(inside, _)| inside.to_owned()),
        _ => Some(CLAUSE_END.find(after).map_or(after, |end| &after[..end.start()]).trim().to_owned()),
    };
    quoted.filter(|text| !text.trim().is_empty())
}

pub fn expected(text: &str) -> Expected {
    let text = text.trim();
    if text.is_empty() {
        return Expected::Judge("no stated expectation".into());
    }
    if let Some(found) = THRESHOLD.find(text) {
        return Expected::Judge(format!("a threshold the program cannot read ({})", found.as_str()));
    }
    let mut all = Vec::new();
    let mut rest = text.to_owned();
    if let Some(found) = EMPTY.find(&rest) {
        all.push(Expectation::Empty);
        rest.replace_range(found.range(), " ");
    }
    for found in SHOWS.find_iter(&rest) {
        match shown(&rest[found.end()..]) {
            Some(shown) => all.push(Expectation::Contains(shown)),
            None => return Expected::Judge(format!("nothing named after `{}`", found.as_str().trim())),
        }
    }
    if let Some(code) = EXIT.captures(&rest).and_then(|found| found[3].parse().ok()) {
        all.push(Expectation::Exit(code));
    } else if PASSES.is_match(&rest) {
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
