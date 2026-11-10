use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::map::{Function, Index, Item, Refers};

const DEFINITION: u64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub line: u32,
    pub symbol: String,
    pub definition: bool,
    pub enclosing: Option<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub path: String,
    pub occurrences: Vec<Occurrence>,
}

struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

enum Value<'b> {
    Number(u64),
    Bytes(&'b [u8]),
    Skipped,
}

impl<'b> Reader<'b> {
    fn new(bytes: &'b [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or("the SCIP index ends inside a number")?;
            self.at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(value);
            }
        }
        Err("the SCIP index holds a number longer than 64 bits".to_owned())
    }

    fn take(&mut self, length: usize) -> Result<&'b [u8], String> {
        let end = self.at.checked_add(length).filter(|end| *end <= self.bytes.len()).ok_or("the SCIP index ends inside a field")?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn field(&mut self) -> Result<Option<(u64, Value<'b>)>, String> {
        if self.at >= self.bytes.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let value = match key & 7 {
            0 => Value::Number(self.varint()?),
            1 => self.take(8).map(|_| Value::Skipped)?,
            2 => {
                let length = usize::try_from(self.varint()?).map_err(|_| "a SCIP field is too long")?;
                Value::Bytes(self.take(length)?)
            }
            5 => self.take(4).map(|_| Value::Skipped)?,
            wire => return Err(format!("the SCIP index uses protobuf wire type {wire}, which it never writes")),
        };
        Ok(Some((key >> 3, value)))
    }
}

fn packed(bytes: &[u8]) -> Result<Vec<u64>, String> {
    let mut reader = Reader::new(bytes);
    let mut numbers = Vec::new();
    while reader.at < bytes.len() {
        numbers.push(reader.varint()?);
    }
    Ok(numbers)
}

fn line_span(range: &[u64]) -> Option<(u32, u32)> {
    let line = |value: u64| u32::try_from(value).ok()?.checked_add(1);
    match range {
        [start, _, _] => Some((line(*start)?, line(*start)?)),
        [start, _, end, _] => Some((line(*start)?, line(*end)?)),
        _ => None,
    }
}

fn occurrence(bytes: &[u8]) -> Result<Option<Occurrence>, String> {
    let mut reader = Reader::new(bytes);
    let (mut range, mut symbol, mut roles, mut enclosing) = (Vec::new(), String::new(), 0, None);
    while let Some((number, value)) = reader.field()? {
        match (number, value) {
            (1, Value::Bytes(bytes)) => range = packed(bytes)?,
            (2, Value::Bytes(bytes)) => symbol = String::from_utf8_lossy(bytes).into_owned(),
            (3, Value::Number(value)) => roles = value,
            (7, Value::Bytes(bytes)) => enclosing = line_span(&packed(bytes)?),
            _ => {}
        }
    }
    let Some((line, _)) = line_span(&range) else { return Ok(None) };
    if symbol.is_empty() || symbol.starts_with("local ") {
        return Ok(None);
    }
    Ok(Some(Occurrence { line, symbol, definition: roles & DEFINITION != 0, enclosing }))
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Document>, String> {
    let mut reader = Reader::new(bytes);
    let mut documents = Vec::new();
    while let Some((number, value)) = reader.field()? {
        let (2, Value::Bytes(document)) = (number, value) else { continue };
        let mut fields = Reader::new(document);
        let mut decoded = Document { path: String::new(), occurrences: Vec::new() };
        while let Some((number, value)) = fields.field()? {
            match (number, value) {
                (1, Value::Bytes(path)) => decoded.path = String::from_utf8_lossy(path).into_owned(),
                (2, Value::Bytes(bytes)) => decoded.occurrences.extend(occurrence(bytes)?),
                _ => {}
            }
        }
        documents.push(decoded);
    }
    Ok(documents)
}

fn innermost(spans: &[(u32, u32, usize)], line: u32) -> Option<usize> {
    spans.iter().filter(|(start, end, _)| *start <= line && line <= *end).min_by_key(|(start, end, _)| end - start).map(|(_, _, index)| *index)
}

fn opens_use(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix("pub").map_or(line, |rest| rest.trim_start().strip_prefix('(').and_then(|rest| rest.split_once(')')).map_or(rest, |(_, rest)| rest)).trim_start();
    line.starts_with("use ")
}

fn decorates(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("#[") || line.starts_with("///") || line.starts_with("//!")
}

fn opens_impl(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix("unsafe ").unwrap_or(line);
    line.starts_with("impl ") || line.starts_with("impl<")
}

struct Symbols {
    names: Vec<String>,
    numbers: BTreeMap<String, u32>,
}

impl Symbols {
    fn number(&mut self, symbol: &str) -> u32 {
        if let Some(number) = self.numbers.get(symbol) {
            return *number;
        }
        let number = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.push(symbol.to_owned());
        self.numbers.insert(symbol.to_owned(), number);
        number
    }
}

struct Builder {
    symbols: Symbols,
    items: Vec<Item>,
    refers: BTreeMap<u32, BTreeSet<u32>>,
}

struct Source<'t> {
    path: &'t str,
    lines: Vec<&'t str>,
    regions: Vec<(u32, u32, usize)>,
}

impl Source<'_> {
    fn line(&self, line: u32) -> &str {
        line.checked_sub(1).and_then(|index| self.lines.get(index as usize)).copied().unwrap_or("")
    }

    fn last(&self) -> u32 {
        u32::try_from(self.lines.len()).unwrap_or(u32::MAX)
    }
}

impl Builder {
    fn definitions(&mut self, source: &Source, occurrences: &[Occurrence]) {
        for occurrence in occurrences.iter().filter(|occurrence| occurrence.definition && !occurrence.symbol.ends_with('/')) {
            let Some((mut start, end)) = occurrence.enclosing else { continue };
            while start > 1 && decorates(source.line(start - 1)) {
                start -= 1;
            }
            let defines = vec![self.symbols.number(&occurrence.symbol)];
            self.items.push(Item { file: source.path.to_owned(), start, end, defines, uses: Vec::new() });
        }
    }

    fn imports(&mut self, source: &Source) {
        let mut line = 1;
        while line <= source.last() {
            if opens_use(source.line(line)) && innermost(&source.regions, line).is_none() {
                let start = line;
                while !source.line(line).contains(';') && line < source.last() {
                    line += 1;
                }
                self.items.push(Item { file: source.path.to_owned(), start, end: line, defines: Vec::new(), uses: Vec::new() });
            }
            line += 1;
        }
    }

    fn references(&mut self, source: &Source, occurrences: &[Occurrence], first: usize) {
        let spans: Vec<(u32, u32, usize)> = self.items[first..].iter().enumerate().map(|(offset, item)| (item.start, item.end, first + offset)).collect();
        let mut headers: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for occurrence in occurrences.iter().filter(|occurrence| !occurrence.definition) {
            let symbol = self.symbols.number(&occurrence.symbol);
            if let Some(function) = innermost(&source.regions, occurrence.line) {
                self.refers.entry(u32::try_from(function).unwrap_or(u32::MAX)).or_default().insert(symbol);
            } else if let Some(item) = innermost(&spans, occurrence.line) {
                if !self.items[item].defines.is_empty() {
                    self.items[item].uses.push(symbol);
                }
            } else if opens_impl(source.line(occurrence.line)) {
                headers.entry(occurrence.line).or_default().insert(symbol);
            }
        }
        for (line, referenced) in headers {
            self.items.push(Item { file: source.path.to_owned(), start: line, end: line, defines: referenced.into_iter().collect(), uses: Vec::new() });
        }
    }
}

pub fn build(documents: &[Document], functions: &[Function], source: impl Fn(&str) -> Option<String>) -> Index {
    let mut builder = Builder { symbols: Symbols { names: Vec::new(), numbers: BTreeMap::new() }, items: Vec::new(), refers: BTreeMap::new() };
    for document in documents {
        let Some(text) = source(&document.path) else { continue };
        let regions = functions.iter().enumerate().filter(|(_, function)| function.file == document.path).map(|(index, function)| (function.start, function.end, index)).collect();
        let source = Source { path: &document.path, lines: text.lines().collect(), regions };
        let first = builder.items.len();
        builder.definitions(&source, &document.occurrences);
        builder.imports(&source);
        builder.references(&source, &document.occurrences, first);
    }
    for item in &mut builder.items {
        item.uses.sort_unstable();
        item.uses.dedup();
    }
    let refers = builder.refers.into_iter().map(|(function, symbols)| Refers { function, symbols: symbols.into_iter().collect() }).collect();
    Index { symbols: builder.symbols.names, items: builder.items, refers }
}

pub fn record(root: &Path, work: &Path, functions: &[Function]) -> Result<Index, String> {
    let analyzer = std::env::var_os("NORTH_RUST_ANALYZER").unwrap_or_else(|| "rust-analyzer".into());
    let output = work.join("index.scip");
    let ran = Command::new(&analyzer).arg("scip").arg(root).arg("--output").arg(&output).current_dir(root).output().map_err(|why| format!("cannot run {}: {why}", Path::new(&analyzer).display()))?;
    if !ran.status.success() {
        return Err(format!("rust-analyzer scip failed: {}", String::from_utf8_lossy(&ran.stderr).lines().rev().take(3).collect::<Vec<_>>().join(" | ")));
    }
    let bytes = std::fs::read(&output).map_err(|why| format!("cannot read {}: {why}", output.display()))?;
    let documents = decode(&bytes)?;
    Ok(build(&documents, functions, |path| std::fs::read_to_string(root.join(path)).ok()))
}
