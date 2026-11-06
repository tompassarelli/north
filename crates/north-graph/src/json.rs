use serde_json::{Value, json};

use crate::term::{Atom, Term};

pub fn term_to_json(term: &Term) -> Value {
    match term {
        Term::Atom(Atom::String(text)) => json!(["string", text]),
        Term::Atom(Atom::Int(value)) => json!(["integer", value.to_string()]),
        Term::Atom(Atom::Float(value)) => json!(["float64", format!("{:?}", value.value())]),
        Term::Atom(Atom::Bool(value)) => json!(["boolean", value]),
        Term::Atom(Atom::Keyword(name)) => json!(["keyword", name]),
        Term::Atom(Atom::Instant(instant)) => json!(["instant", instant.seconds().to_string(), instant.nanos().to_string()]),
        Term::Triple(triple) => json!(["triple", term_to_json(&triple.t1), term_to_json(&triple.t2), term_to_json(&triple.t3)]),
    }
}

pub fn term_from_json(value: &Value) -> Result<Term, String> {
    match value {
        Value::String(text) => Ok(Term::string(text.as_str())),
        Value::Bool(flag) => Ok(Term::boolean(*flag)),
        Value::Number(number) => number.as_i64().map(Term::int).or_else(|| number.as_f64().map(Term::float)).ok_or_else(|| format!("number {number} is out of range")),
        Value::Array(items) => tagged(items),
        other => Err(format!("{other} is not a term")),
    }
}

fn tagged(items: &[Value]) -> Result<Term, String> {
    let tag = items.first().and_then(Value::as_str).ok_or("a tagged term starts with its kind")?;
    let args = &items[1..];
    match (tag, args) {
        ("string", [Value::String(text)]) => Ok(Term::string(text.as_str())),
        ("integer", [value]) => integer(value).map(Term::int),
        ("float64", [value]) => float(value),
        ("boolean", [Value::Bool(flag)]) => Ok(Term::boolean(*flag)),
        ("keyword", [Value::String(name)]) => Ok(Term::keyword(name.as_str())),
        ("instant", [seconds, nanos]) => {
            let nanos = integer(nanos)?;
            Term::instant(integer(seconds)?, u32::try_from(nanos).map_err(|_| format!("nanos {nanos} out of range"))?).map_err(|why| why.to_string())
        }
        ("triple", [t1, t2, t3]) => Ok(Term::triple(term_from_json(t1)?, term_from_json(t2)?, term_from_json(t3)?)),
        _ => Err(format!("bad {tag} term: {} argument(s)", args.len())),
    }
}

fn integer(value: &Value) -> Result<i64, String> {
    match value {
        Value::String(text) => text.parse().map_err(|_| format!("{text} is not a 64-bit integer")),
        Value::Number(number) => number.as_i64().ok_or_else(|| format!("{number} is not a 64-bit integer")),
        other => Err(format!("{other} is not an integer")),
    }
}

fn float(value: &Value) -> Result<Term, String> {
    match value {
        Value::String(text) => text.parse::<f64>().map(Term::float).map_err(|_| format!("{text} is not a float")),
        Value::Number(number) => number.as_f64().map(Term::float).ok_or_else(|| format!("{number} is not a float")),
        other => Err(format!("{other} is not a float")),
    }
}
