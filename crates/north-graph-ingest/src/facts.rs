use north_graph::{Telling, Term, Told};

pub struct Facts {
    telling: Telling,
    notes: Vec<String>,
}

pub struct Sourced {
    pub telling: Telling,
    pub notes: Vec<String>,
}

impl From<Telling> for Sourced {
    fn from(telling: Telling) -> Self {
        Self { telling, notes: Vec::new() }
    }
}

impl Facts {
    pub fn new(replaces: &[&str]) -> Self {
        Self { telling: Telling { facts: Vec::new(), replaces: replaces.iter().map(|name| Term::keyword(*name)).collect(), ..Telling::default() }, notes: Vec::new() }
    }

    pub fn tell(&mut self, writer: &str, subject: impl Into<Term>, predicate: &str, object: impl Into<Term>) {
        self.telling.facts.push(Told::new(writer, subject, Term::keyword(predicate), object));
    }

    pub fn note(&mut self, line: String) {
        self.notes.push(line);
    }

    pub fn finish(self) -> Telling {
        self.telling
    }

    pub fn finish_noted(self) -> Sourced {
        Sourced { telling: self.telling, notes: self.notes }
    }
}

pub fn text(value: impl Into<String>) -> Term {
    Term::string(value)
}
