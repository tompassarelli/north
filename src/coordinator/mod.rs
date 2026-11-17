//! The kinds of verb a coordinator may run, which the generated verb table and the guard that reads it share.

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Delegate,
    ChildMessage,
    Brief,
    Bookkeeping,
    Landing,
    Status,
    Diagnosis,
}

pub struct VerbKinds(&'static [(&'static str, Kind)]);

impl VerbKinds {
    pub const fn new(table: &'static [(&'static str, Kind)]) -> Self {
        Self(table)
    }

    pub fn kind(&self, words: &[&str]) -> Option<Kind> {
        let Some(verb) = words.first() else {
            return self.declared("");
        };
        words.get(1).and_then(|sub| self.declared(&format!("{verb} {sub}"))).or_else(|| self.declared(verb))
    }

    fn declared(&self, path: &str) -> Option<Kind> {
        self.0.iter().find(|(known, _)| *known == path).map(|(_, kind)| *kind)
    }
}

pub fn run(arguments: &[String], role: fn(&[String]) -> u8) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "role" => role(rest),
        _ => {
            eprintln!("usage: north config coordinator role");
            2
        }
    }
}
