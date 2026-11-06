use std::fmt;

use crate::term::{Atom, Float, Instant, MAX_TERM_DEPTH, Term, Triple};

impl fmt::Display for Atom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(text) => write!(formatter, "{}", serde_json::Value::String(text.clone())),
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(value) => write_float(formatter, *value),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Keyword(name) => write!(formatter, ":{name}"),
            Self::Instant(instant) => write!(formatter, "@{}.{:09}", instant.seconds(), instant.nanos()),
        }
    }
}

fn write_float(formatter: &mut fmt::Formatter<'_>, value: Float) -> fmt::Result {
    let number = value.value();
    if number.is_nan() {
        write!(formatter, "NaN")
    } else if number.is_infinite() {
        write!(formatter, "{}", if number > 0.0 { "Infinity" } else { "-Infinity" })
    } else {
        write!(formatter, "{number:?}")
    }
}

impl fmt::Display for Term {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atom(atom) => atom.fmt(formatter),
            Self::Triple(triple) => triple.fmt(formatter),
        }
    }
}

impl fmt::Display for Triple {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({} {} {})", self.t1, self.t2, self.t3)
    }
}

pub fn parse_arg(text: &str) -> Result<Term, String> {
    let text = text.trim();
    if text.starts_with(['(', '"', '@']) {
        let mut cursor = Cursor { rest: text, depth: 0 };
        let term = cursor.term()?;
        cursor.skip_space();
        return if cursor.rest.is_empty() { Ok(term) } else { Err(format!("unexpected text after term: {}", cursor.rest)) };
    }
    Ok(bare(text))
}

fn bare(token: &str) -> Term {
    match token {
        "true" => return Term::boolean(true),
        "false" => return Term::boolean(false),
        "NaN" => return Term::float(f64::NAN),
        "Infinity" => return Term::float(f64::INFINITY),
        "-Infinity" => return Term::float(f64::NEG_INFINITY),
        _ => {}
    }
    if let Ok(value) = token.parse::<i64>() {
        return Term::int(value);
    }
    let numeric = token.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.');
    if numeric && let Ok(value) = token.parse::<f64>() && value.is_finite() {
        return Term::float(value);
    }
    match token.strip_prefix(':') {
        Some(name) if !name.is_empty() && !name.contains(char::is_whitespace) => Term::keyword(name),
        _ => Term::string(token),
    }
}

struct Cursor<'a> {
    rest: &'a str,
    depth: usize,
}

impl Cursor<'_> {
    fn skip_space(&mut self) {
        self.rest = self.rest.trim_start();
    }

    fn term(&mut self) -> Result<Term, String> {
        self.skip_space();
        match self.rest.chars().next() {
            None => Err("expected a term".to_string()),
            Some('(') => self.nested(),
            Some('"') => self.string(),
            Some('@') => self.instant(),
            Some(')') => Err("unexpected )".to_string()),
            Some(_) => Ok(bare(self.token())),
        }
    }

    fn token(&mut self) -> &str {
        let end = self.rest.find(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '"').unwrap_or(self.rest.len());
        let (token, rest) = self.rest.split_at(end);
        self.rest = rest;
        token
    }

    fn nested(&mut self) -> Result<Term, String> {
        self.depth += 1;
        let parsed = if self.depth > MAX_TERM_DEPTH { Err(format!("a term nests deeper than {MAX_TERM_DEPTH}")) } else { self.triple() };
        self.depth -= 1;
        parsed
    }

    fn triple(&mut self) -> Result<Term, String> {
        self.rest = &self.rest[1..];
        let mut parts = Vec::with_capacity(3);
        loop {
            self.skip_space();
            if let Some(rest) = self.rest.strip_prefix(')') {
                self.rest = rest;
                break;
            }
            parts.push(self.term()?);
        }
        let [t1, t2, t3]: [Term; 3] = parts.try_into().map_err(|parts: Vec<Term>| format!("a triple has exactly three terms, found {}", parts.len()))?;
        Ok(Term::triple(t1, t2, t3))
    }

    fn string(&mut self) -> Result<Term, String> {
        let mut stream = serde_json::Deserializer::from_str(self.rest).into_iter::<String>();
        let value = stream.next().ok_or("expected a string")?.map_err(|why| format!("bad string: {why}"))?;
        self.rest = &self.rest[stream.byte_offset()..];
        Ok(Term::string(value))
    }

    fn instant(&mut self) -> Result<Term, String> {
        let token = self.token().to_string();
        let bad = || format!("bad instant {token}: expected @SECONDS.NNNNNNNNN");
        let (seconds, nanos) = token[1..].split_once('.').ok_or_else(bad)?;
        let seconds = seconds.parse::<i64>().map_err(|_| bad())?;
        if nanos.len() != 9 {
            return Err(bad());
        }
        let nanos = nanos.parse::<u32>().map_err(|_| bad())?;
        Instant::new(seconds, nanos).map(|instant| Term::Atom(Atom::Instant(instant))).map_err(|why| why.to_string())
    }
}
