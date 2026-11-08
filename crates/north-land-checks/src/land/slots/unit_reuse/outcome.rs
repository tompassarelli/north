use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub binary: String,
    pub test: String,
    pub passed: bool,
    pub seconds: f64,
}

const PASSED: [&str; 1] = ["PASS"];
const FAILED: [&str; 2] = ["FAIL", "TIMEOUT"];

fn verdict(word: &str) -> Option<bool> {
    if PASSED.contains(&word) {
        Some(true)
    } else if FAILED.contains(&word) || word.starts_with("SIG") {
        Some(false)
    } else {
        None
    }
}

fn line(text: &str) -> Option<Outcome> {
    let text = text.trim_start();
    let (status, rest) = text.split_once(' ')?;
    let passed = verdict(status)?;
    let (timing, rest) = rest.trim_start().strip_prefix('[')?.split_once(']')?;
    let seconds = timing.trim().strip_suffix('s')?.parse().ok()?;
    let mut words = rest.split_whitespace().peekable();
    if words.peek().is_some_and(|word| word.starts_with('(')) {
        words.by_ref().find(|word| word.ends_with(')'));
    }
    let (binary, test) = (words.next()?, words.next()?);
    words.next().is_none().then(|| Outcome { binary: binary.to_owned(), test: test.to_owned(), passed, seconds })
}

pub fn parse(text: &str) -> Vec<Outcome> {
    let mut first: BTreeMap<(String, String), Outcome> = BTreeMap::new();
    for outcome in text.lines().filter_map(line) {
        first.entry((outcome.binary.clone(), outcome.test.clone())).or_insert(outcome);
    }
    first.into_values().collect()
}
