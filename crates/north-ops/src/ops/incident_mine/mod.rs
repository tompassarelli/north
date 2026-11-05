mod classify;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use classify::{Hit, Message, assistant_candidate, excerpt, user_candidate};

const USAGE: &str = "usage: north incident-mine [--days N] [--chunks N] [--per-thread N] [--out DIR]";

struct Options {
    days: u32,
    chunks: usize,
    per_thread: usize,
    out: PathBuf,
}

#[derive(Serialize)]
struct Row {
    #[serde(rename = "ref")]
    reference: serde_json::Value,
    provider: String,
    session: String,
    project: Option<String>,
    ts: String,
    role: String,
    source: &'static str,
    flag: &'static str,
    text: String,
}

struct Chunk {
    path: PathBuf,
    rows: usize,
    tokens: usize,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut options = Options {
        days: 7,
        chunks: 6,
        per_thread: 3,
        out: home.join(".local/state/agents/policy-incidents/chunks"),
    };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag.as_str(), None),
        };
        let value = inline.or_else(|| rest.next().map(String::as_str))?;
        match name {
            "--days" => options.days = value.parse().ok()?,
            "--chunks" => options.chunks = value.parse().ok().filter(|chunks| *chunks > 0)?,
            "--per-thread" => options.per_thread = value.parse().ok()?,
            "--out" => options.out = PathBuf::from(value),
            _ => return None,
        }
    }
    Some(options)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match mine(&options) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("incident-mine: {error}");
            1
        }
    }
}

fn convo(role: &str, provider: &str, days: u32) -> Result<Vec<Message>, String> {
    let arguments = [
        "-r", role, "-m", "--since", &format!("{days}d"), "-n", "1000000", "--json", "--full", "--provider", provider,
    ];
    let output = Command::new("north").arg("convo").args(arguments).output().map_err(|error| format!("north convo: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "convo failed ({}): north convo {}\n{}",
            output.status.code().map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stdout.trim_ascii_start().starts_with(b"[") {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("convo output: {error}"))
}

fn collect(options: &Options) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut per_thread: HashMap<(String, Option<String>, &str), usize> = HashMap::new();
    for provider in ["claude", "codex"] {
        let mut hits: Vec<(Message, Option<Hit>)> = Vec::new();
        for (role, classify) in [("user", user_candidate as fn(&Message) -> Option<Hit>), ("assistant", assistant_candidate)] {
            for message in convo(role, provider, options.days)? {
                let hit = classify(&message);
                hits.push((message, hit));
            }
        }
        for (message, hit) in hits {
            let Some((source, flag)) = hit else { continue };
            let row = row(&message, source, flag, if source == "tom-typed" { 360 } else { 220 });
            let key = (source, row.text.chars().take(200).collect::<String>().to_lowercase());
            let thread = (message.session.clone(), message.agent.clone(), flag);
            let count = per_thread.get(&thread).copied().unwrap_or(0);
            if seen.contains(&key) || (source == "assistant" && count >= options.per_thread) {
                continue;
            }
            seen.insert(key);
            *per_thread.entry(thread).or_default() += 1;
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| a.ts.cmp(&b.ts));
    Ok(rows)
}

fn row(message: &Message, source: &'static str, flag: &'static str, limit: usize) -> Row {
    Row {
        reference: message.id.clone(),
        provider: message.provider.clone(),
        session: message.session.clone(),
        project: message.project.clone(),
        ts: message.ts.clone(),
        role: message.role.clone(),
        source,
        flag,
        text: excerpt(message.text(), limit),
    }
}

struct PythonSeparators;

impl serde_json::ser::Formatter for PythonSeparators {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }
}

fn line(row: &Row) -> String {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, PythonSeparators);
    row.serialize(&mut serializer).expect("a row serializes");
    String::from_utf8(bytes).expect("serde_json writes UTF-8")
}

fn split(lines: &[String], chunks: usize) -> Vec<&[String]> {
    let total: usize = lines.iter().map(|line| line.chars().count()).sum();
    let budget = total as f64 / chunks as f64 + 1.0;
    let mut groups = Vec::new();
    let (mut start, mut size) = (0, 0usize);
    for (index, line) in lines.iter().enumerate() {
        let length = line.chars().count();
        if index > start && (size + length) as f64 > budget && groups.len() + 1 < chunks {
            groups.push(&lines[start..index]);
            (start, size) = (index, 0);
        }
        size += length;
    }
    if start < lines.len() {
        groups.push(&lines[start..]);
    }
    groups
}

fn write_chunks(out: &Path, lines: &[String], chunks: usize) -> io::Result<Vec<Chunk>> {
    fs::create_dir_all(out)?;
    for entry in fs::read_dir(out)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("chunk-") && name.ends_with(".jsonl") {
            fs::remove_file(entry.path())?;
        }
    }
    let mut written = Vec::new();
    for (index, group) in split(lines, chunks).into_iter().enumerate() {
        let path = out.join(format!("chunk-{}.jsonl", index + 1));
        fs::write(&path, group.join("\n") + "\n")?;
        written.push(Chunk { path, rows: group.len(), tokens: group.iter().map(|line| line.chars().count()).sum::<usize>() / 4 });
    }
    Ok(written)
}

fn tally<K: Ord>(rows: &[Row], key: impl Fn(&Row) -> K) -> BTreeMap<K, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(key(row)).or_default() += 1;
    }
    counts
}

fn mine(options: &Options) -> Result<(), String> {
    let rows = collect(options)?;
    let lines: Vec<String> = rows.iter().map(line).collect();
    let total: usize = lines.iter().map(|line| line.chars().count()).sum();
    let written = write_chunks(&options.out, &lines, options.chunks).map_err(|error| format!("{}: {error}", options.out.display()))?;
    println!("candidates: {}  (~{} tokens)", rows.len(), total / 4);
    for (source, count) in tally(&rows, |row| row.source) {
        println!("  source {source}: {count}");
    }
    for (provider, count) in tally(&rows, |row| row.provider.clone()) {
        println!("  provider {provider}: {count}");
    }
    for ((provider, source, flag), count) in tally(&rows, |row| (row.provider.clone(), row.source, row.flag)) {
        println!("  {provider}/{source}/{flag}: {count}");
    }
    for chunk in written {
        println!("chunk {}: {} rows, ~{} tokens", chunk.path.display(), chunk.rows, chunk.tokens);
    }
    Ok(())
}
