use std::cmp::Ordering;

const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;
const NANOS_PER_SECOND: u32 = 1_000_000_000;
pub const MAX_TERM_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Float(u64);

impl Float {
    pub fn new(value: f64) -> Self {
        Self(if value.is_nan() { CANONICAL_NAN } else { value.to_bits() })
    }

    pub fn from_bits(bits: u64) -> Self {
        Self::new(f64::from_bits(bits))
    }

    pub fn bits(self) -> u64 {
        self.0
    }

    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl Ord for Float {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value().total_cmp(&other.value())
    }
}

impl PartialOrd for Float {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instant {
    seconds: i64,
    nanos: u32,
}

impl Instant {
    pub fn new(seconds: i64, nanos: u32) -> Result<Self, TermError> {
        if nanos >= NANOS_PER_SECOND {
            return Err(TermError::InvalidInstant { seconds, nanos });
        }
        Ok(Self { seconds, nanos })
    }

    pub fn seconds(self) -> i64 {
        self.seconds
    }

    pub fn nanos(self) -> u32 {
        self.nanos
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Atom {
    String(String),
    Int(i64),
    Float(Float),
    Bool(bool),
    Keyword(String),
    Instant(Instant),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Term {
    Atom(Atom),
    Triple(Box<Triple>),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Triple {
    pub t1: Term,
    pub t2: Term,
    pub t3: Term,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TermError {
    InvalidInstant { seconds: i64, nanos: u32 },
    InvalidKey(String),
}

impl std::fmt::Display for TermError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInstant { seconds, nanos } => write!(formatter, "invalid instant {seconds}s {nanos}ns: nanos must be below one second"),
            Self::InvalidKey(why) => write!(formatter, "invalid stored term: {why}"),
        }
    }
}

impl std::error::Error for TermError {}

impl Triple {
    pub fn depth(&self) -> usize {
        1 + [&self.t1, &self.t2, &self.t3].into_iter().map(Term::depth).max().unwrap_or(0)
    }

    pub fn new(t1: impl Into<Term>, t2: impl Into<Term>, t3: impl Into<Term>) -> Self {
        Self { t1: t1.into(), t2: t2.into(), t3: t3.into() }
    }
}

impl Term {
    pub fn string(value: impl Into<String>) -> Self {
        Self::Atom(Atom::String(value.into()))
    }

    pub fn keyword(name: impl Into<String>) -> Self {
        Self::Atom(Atom::Keyword(name.into()))
    }

    pub fn int(value: i64) -> Self {
        Self::Atom(Atom::Int(value))
    }

    pub fn float(value: f64) -> Self {
        Self::Atom(Atom::Float(Float::new(value)))
    }

    pub fn boolean(value: bool) -> Self {
        Self::Atom(Atom::Bool(value))
    }

    pub fn instant(seconds: i64, nanos: u32) -> Result<Self, TermError> {
        Ok(Self::Atom(Atom::Instant(Instant::new(seconds, nanos)?)))
    }

    pub fn triple(t1: impl Into<Term>, t2: impl Into<Term>, t3: impl Into<Term>) -> Self {
        Self::Triple(Box::new(Triple::new(t1, t2, t3)))
    }

    pub fn depth(&self) -> usize {
        match self {
            Self::Atom(_) => 1,
            Self::Triple(triple) => triple.depth(),
        }
    }

    pub fn as_triple(&self) -> Option<&Triple> {
        match self {
            Self::Triple(triple) => Some(triple),
            Self::Atom(_) => None,
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Atom(Atom::Int(_) | Atom::Float(_)))
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Atom(Atom::String(_)) => 0,
            Self::Atom(Atom::Int(_)) => 1,
            Self::Atom(Atom::Float(_)) => 2,
            Self::Atom(Atom::Bool(_)) => 3,
            Self::Atom(Atom::Keyword(_)) => 4,
            Self::Atom(Atom::Instant(_)) => 5,
            Self::Triple(_) => 6,
        }
    }
}

impl From<Triple> for Term {
    fn from(triple: Triple) -> Self {
        Self::Triple(Box::new(triple))
    }
}

impl From<Atom> for Term {
    fn from(atom: Atom) -> Self {
        Self::Atom(atom)
    }
}

impl From<&str> for Term {
    fn from(value: &str) -> Self {
        Self::string(value)
    }
}

impl From<i64> for Term {
    fn from(value: i64) -> Self {
        Self::int(value)
    }
}

impl From<bool> for Term {
    fn from(value: bool) -> Self {
        Self::boolean(value)
    }
}

impl Ord for Term {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Atom(left), Self::Atom(right)) if self.rank() == other.rank() => left.cmp(right),
            (Self::Triple(left), Self::Triple(right)) => left.cmp(right),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Term {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn numeric_cmp(left: &Term, right: &Term) -> Option<Ordering> {
    match (left, right) {
        (Term::Atom(Atom::Int(a)), Term::Atom(Atom::Int(b))) => Some(a.cmp(b)),
        (Term::Atom(Atom::Int(a)), Term::Atom(Atom::Float(b))) => (*a as f64).partial_cmp(&b.value()),
        (Term::Atom(Atom::Float(a)), Term::Atom(Atom::Int(b))) => a.value().partial_cmp(&(*b as f64)),
        (Term::Atom(Atom::Float(a)), Term::Atom(Atom::Float(b))) => a.value().partial_cmp(&b.value()),
        _ => None,
    }
}
