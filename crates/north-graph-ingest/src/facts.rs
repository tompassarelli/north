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
        Self { telling: Telling { facts: Vec::new(), replaces: replaces.iter().map(|name| Term::keyword(*name)).collect() }, notes: Vec::new() }
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

pub fn keyword(name: &str) -> Term {
    Term::keyword(name)
}

pub fn instant(seconds: f64) -> Option<Term> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = seconds.floor();
    let nanos = ((seconds - whole) * 1e9).floor().clamp(0.0, 999_999_999.0);
    Term::instant(whole as i64, nanos as u32).ok()
}

pub fn issue_ref(reference: &str) -> Option<String> {
    let (repo, number) = reference.rsplit_once('#')?;
    number.parse::<u64>().ok()?;
    let name = repo.rsplit('/').next().filter(|name| !name.is_empty())?;
    Some(format!("{name}#{number}"))
}
