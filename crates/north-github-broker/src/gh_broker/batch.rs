use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::http::{Request, Response};

type Bucket = [u8; 32];

#[derive(Clone)]
pub struct Part {
    query: String,
    variables: Map<String, Value>,
}

impl Part {
    fn of(request: &Request) -> Option<Self> {
        if request.method != "POST" || request.target.split('?').next() != Some("/graphql") {
            return None;
        }
        let body: Value = serde_json::from_slice(&request.body).ok()?;
        let object = body.as_object()?;
        if object.keys().any(|key| !["query", "variables"].contains(&key.as_str())) {
            return None;
        }
        let variables = match object.get("variables") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(variables)) => variables.clone(),
            Some(_) => return None,
        };
        Some(Self { query: object.get("query")?.as_str()?.to_owned(), variables })
    }
}

#[derive(Debug, PartialEq)]
enum Token<'q> {
    Name(&'q str),
    Variable(&'q str),
    Punct(char),
    Value,
}

type Tokens<'q> = Vec<(usize, usize, Token<'q>)>;

fn is_name(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn skip_while(bytes: &[u8], mut at: usize, keep: impl Fn(u8) -> bool) -> usize {
    while at < bytes.len() && keep(bytes[at]) {
        at += 1;
    }
    at
}

fn string_end(query: &str, from: usize) -> Option<usize> {
    let close = if query[from..].starts_with("\"\"\"") { "\"\"\"" } else { "\"" };
    let mut at = from + close.len();
    loop {
        let rest = query.get(at..)?;
        if rest.starts_with('\\') {
            at += 2;
        } else if rest.starts_with(close) {
            return Some(at + close.len());
        } else {
            at += rest.chars().next()?.len_utf8();
        }
    }
}

fn tokens(query: &str) -> Option<Tokens<'_>> {
    let bytes = query.as_bytes();
    let (mut at, mut found) = (0, Vec::new());
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        let token = match byte {
            b' ' | b'\t' | b'\n' | b'\r' | b',' => {
                at += 1;
                continue;
            }
            b'#' => {
                at = skip_while(bytes, at, |byte| byte != b'\n');
                continue;
            }
            b'"' => {
                at = string_end(query, at)?;
                Token::Value
            }
            b'$' => {
                at = skip_while(bytes, at + 1, is_name);
                Token::Variable(&query[start + 1..at])
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                at = skip_while(bytes, at, is_name);
                Token::Name(&query[start..at])
            }
            byte if byte.is_ascii_digit() || byte == b'-' => {
                at = skip_while(bytes, at + 1, |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'));
                Token::Value
            }
            b'.' | b'@' => return None,
            byte if byte.is_ascii() => {
                at += 1;
                Token::Punct(byte as char)
            }
            _ => return None,
        };
        found.push((start, at, token));
    }
    Some(found)
}

fn closing(found: &Tokens<'_>, open: usize) -> Option<usize> {
    let (opening, closer) = match found.get(open)?.2 {
        Token::Punct('(') => ('(', ')'),
        Token::Punct('{') => ('{', '}'),
        _ => return None,
    };
    let mut depth = 0;
    for (index, (_, _, token)) in found.iter().enumerate().skip(open) {
        if *token == Token::Punct(opening) {
            depth += 1;
        } else if *token == Token::Punct(closer) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

struct Shape {
    definitions: String,
    fields: String,
    variables: Vec<String>,
}

fn renamed(source: &str, found: &Tokens<'_>, from: usize, to: usize, number: usize, into: &mut String) {
    let mut last = from;
    for (start, stop, token) in found {
        if let Token::Variable(name) = token
            && *start >= from
            && *stop <= to
        {
            into.push_str(&source[last..*start]);
            into.push_str(&format!("${name}_b{number}"));
            last = *stop;
        }
    }
    into.push_str(&source[last..to]);
}

fn shape(part: &Part, number: usize) -> Option<Shape> {
    let found = tokens(&part.query)?;
    if found.iter().any(|(_, _, token)| matches!(token, Token::Name("fragment" | "mutation" | "subscription"))) {
        return None;
    }
    let mut at = 0;
    let mut definitions = (0, 0);
    if matches!(found.first()?.2, Token::Name("query")) {
        at = 1;
        if matches!(found.get(at)?.2, Token::Name(_)) {
            at += 1;
        }
        if found.get(at)?.2 == Token::Punct('(') {
            let close = closing(&found, at)?;
            definitions = (found[at].1, found[close].0);
            at = close + 1;
        }
    }
    if found.get(at)?.2 != Token::Punct('{') {
        return None;
    }
    let end = closing(&found, at)?;
    if end + 1 != found.len() {
        return None;
    }
    let mut renamed_definitions = String::new();
    renamed(&part.query, &found, definitions.0, definitions.1, number, &mut renamed_definitions);
    let mut fields = String::new();
    let mut index = at + 1;
    while index < end {
        let Token::Name(name) = found[index].2 else {
            return None;
        };
        let (mut selected, mut field_end) = (found[index].0, index);
        if found.get(index + 1)?.2 == Token::Punct(':') {
            field_end = index + 2;
            if !matches!(found.get(field_end)?.2, Token::Name(_)) {
                return None;
            }
            selected = found[field_end].0;
        }
        if found.get(field_end + 1).is_some_and(|(_, _, token)| *token == Token::Punct('(')) {
            field_end = closing(&found, field_end + 1)?;
        }
        if found.get(field_end + 1).is_some_and(|(_, _, token)| *token == Token::Punct('{')) {
            field_end = closing(&found, field_end + 1)?;
        }
        fields.push_str(&format!("b{number}_{name}: "));
        renamed(&part.query, &found, selected, found[field_end].1, number, &mut fields);
        fields.push(' ');
        index = field_end + 1;
    }
    let variables = found.iter().filter_map(|(_, _, token)| if let Token::Variable(name) = token { Some((*name).to_owned()) } else { None }).collect();
    Some(Shape { definitions: renamed_definitions, fields, variables })
}

struct Merged {
    request: Request,
    count: usize,
}

fn merge(base: &Request, parts: &[Part]) -> Option<Merged> {
    let (mut definitions, mut fields, mut variables) = (Vec::new(), String::new(), Map::new());
    for (number, part) in parts.iter().enumerate() {
        let shaped = shape(part, number)?;
        if !shaped.definitions.trim().is_empty() {
            definitions.push(shaped.definitions.trim().to_owned());
        }
        fields.push_str(&shaped.fields);
        for name in shaped.variables {
            if let Some(value) = part.variables.get(&name) {
                variables.insert(format!("{name}_b{number}"), value.clone());
            }
        }
    }
    let header = if definitions.is_empty() { String::new() } else { format!("({})", definitions.join(", ")) };
    let body = json!({ "query": format!("query Batch{header} {{ {fields}}}"), "variables": variables });
    Some(Merged { request: Request { body: body.to_string().into_bytes(), ..base.clone() }, count: parts.len() })
}

fn split(merged: &Merged, response: &Response) -> Option<Vec<Response>> {
    if response.status != 200 {
        return Some(vec![response.clone(); merged.count]);
    }
    let body: Value = serde_json::from_slice(&response.body).ok()?;
    let data = body.get("data")?.as_object()?;
    let errors = body.get("errors").and_then(Value::as_array);
    Some(
        (0..merged.count)
            .map(|number| {
                let prefix = format!("b{number}_");
                let own: Map<String, Value> = data.iter().filter_map(|(key, value)| key.strip_prefix(&prefix).map(|name| (name.to_owned(), value.clone()))).collect();
                let mut answer = json!({ "data": own });
                let mine: Vec<Value> = errors
                    .into_iter()
                    .flatten()
                    .filter_map(|error| {
                        let mut error = error.clone();
                        let Some(first) = error.pointer("/path/0").and_then(Value::as_str).map(str::to_owned) else {
                            return Some(error);
                        };
                        error["path"][0] = Value::String(first.strip_prefix(&prefix)?.to_owned());
                        Some(error)
                    })
                    .collect();
                if !mine.is_empty() {
                    answer["errors"] = Value::Array(mine);
                }
                Response { body: answer.to_string().into_bytes(), ..response.clone() }
            })
            .collect(),
    )
}

struct Slot {
    answer: Mutex<Option<Option<Response>>>,
    ready: Condvar,
}

struct Waiting {
    request: Request,
    part: Part,
    slot: Arc<Slot>,
}

pub struct Batcher {
    window: Duration,
    queues: Mutex<HashMap<Bucket, Vec<Waiting>>>,
}

impl Batcher {
    pub fn new(window: Duration) -> Self {
        Self { window, queues: Mutex::default() }
    }

    pub fn send(&self, bucket: Bucket, request: &Request, send: impl Fn(&Request) -> Response) -> (Response, bool) {
        let part = Part::of(request).filter(|part| !self.window.is_zero() && shape(part, 0).is_some());
        let Some(part) = part else {
            return (send(request), false);
        };
        let slot = Arc::new(Slot { answer: Mutex::new(None), ready: Condvar::new() });
        let leads = {
            let mut queues = self.queues.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let queue = queues.entry(bucket).or_default();
            queue.push(Waiting { request: request.clone(), part, slot: Arc::clone(&slot) });
            queue.len() == 1
        };
        if leads {
            std::thread::sleep(self.window);
            let queue = self.queues.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&bucket).unwrap_or_default();
            flush(&queue, &send);
        }
        let mut answer = slot.answer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while answer.is_none() {
            answer = slot.ready.wait(answer).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        match answer.take().flatten() {
            Some(response) => (response, !leads),
            None => (send(request), false),
        }
    }
}

fn flush(queue: &[Waiting], send: &impl Fn(&Request) -> Response) {
    let parts: Vec<Part> = queue.iter().map(|waiting| waiting.part.clone()).collect();
    let mut answers = (queue.len() > 1).then(|| merge(&queue[0].request, &parts)).flatten().and_then(|merged| split(&merged, &send(&merged.request))).map(Vec::into_iter);
    for waiting in queue {
        let answer = answers.as_mut().and_then(Iterator::next);
        *waiting.slot.answer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(answer);
        waiting.slot.ready.notify_all();
    }
}
