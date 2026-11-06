use north_base::foundation::clock::Timestamp;
use north_graph::{Atom, Term, Told};

use crate::reach::Reach;

const CORRECTIONS_SHOWN: usize = 3;
const SESSIONS_READ: usize = 8;

pub struct About(pub Vec<Told>);

pub fn name(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(Atom::String(text)) => Some(text),
        _ => None,
    }
}

fn is(term: &Term, expected: &str) -> bool {
    name(term).is_some_and(|text| text == expected)
}

fn is_keyword(term: &Term, expected: &str) -> bool {
    matches!(term, Term::Atom(Atom::Keyword(text)) if text == expected)
}

impl About {
    pub fn of<'a>(&'a self, subject: &'a str, predicate: &'a str) -> impl Iterator<Item = &'a Term> + 'a {
        self.0.iter().filter(move |told| is(&told.triple.t1, subject) && is_keyword(&told.triple.t2, predicate)).map(|told| &told.triple.t3)
    }

    pub fn first_text<'a>(&'a self, subject: &'a str, predicate: &'a str) -> Option<&'a str> {
        self.of(subject, predicate).find_map(name)
    }

    pub fn texts<'a>(&'a self, subject: &'a str, predicate: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.of(subject, predicate).filter_map(name)
    }

    pub fn when(&self, subject: &str, predicate: &str) -> Option<i64> {
        self.of(subject, predicate).find_map(|term| match term {
            Term::Atom(Atom::Instant(instant)) => Some(instant.seconds()),
            _ => None,
        })
    }

    pub fn subjects<'a>(&'a self, predicate: &'a str, object: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().filter(move |told| is_keyword(&told.triple.t2, predicate) && is(&told.triple.t3, object)).filter_map(|told| name(&told.triple.t1))
    }

    pub fn claims<'a>(&'a self, box_node: &'a str) -> impl Iterator<Item = (&'a str, Option<&'a str>)> + 'a {
        self.0.iter().filter(|told| is_keyword(&told.triple.t2, "claims") && name(&told.triple.t1) == Some(told.writer.as_str())).filter_map(move |told| {
            let done = told.triple.t3.as_triple().filter(|done| is(&done.t1, box_node) && is_keyword(&done.t2, "done-at"))?;
            Some((told.writer.as_str(), name(&done.t3)))
        })
    }

    pub fn observed(&self, commit: &str) -> Option<(bool, &str)> {
        self.0
            .iter()
            .filter(|told| told.writer == "checker" && is(&told.triple.t1, commit))
            .filter_map(|told| {
                let passed = is_keyword(&told.triple.t2, "check-passed");
                (passed || is_keyword(&told.triple.t2, "check-failed")).then(|| name(&told.triple.t3).map(|command| (passed, command)))?
            })
            .max_by_key(|(passed, _)| *passed)
    }
}

pub fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

pub fn cut(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit { flat } else { format!("{}…", flat.chars().take(limit).collect::<String>().trim_end()) }
}

pub fn day(seconds: i64) -> String {
    Timestamp::from_unix_seconds(seconds).iso_date()
}

pub fn corrections(reach: &Reach, sessions: &[&str]) -> Option<String> {
    let mut found: Vec<(i64, String)> = Vec::new();
    for session in sessions.iter().take(SESSIONS_READ) {
        for node in About(reach.about(&Term::string(*session))).subjects("in-session", session) {
            let about = About(reach.about(&Term::string(node)));
            let class = about.first_text(node, "class").map_or("unclassified", |class| class.strip_prefix("class:").unwrap_or(class));
            let paraphrase = about.first_text(node, "paraphrase").unwrap_or_default();
            found.push((about.when(node, "at").unwrap_or_default(), format!("{class}: {}", cut(paraphrase, 110))));
        }
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.dedup();
    let shown: Vec<String> = found.into_iter().take(CORRECTIONS_SHOWN).map(|(_, line)| line).collect();
    (!shown.is_empty()).then(|| format!("  corrections in the sessions that worked on it: {}", shown.join(" | ")))
}
