use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::issue_body::{Gh, Issue, CALL_LIMIT};
use crate::land::after::tick::{self, Tick};

const SHORT_SHA: usize = 12;

fn verdict(body: &str, box_text: &str) -> Result<Option<&'static str>, Tick> {
    match tick::tick(body, box_text, None) {
        Tick::Ticked(_) => Ok(None),
        Tick::NeedsObservation => Ok(Some("the landing will not tick it: it needs observation")),
        Tick::OpenChildren(_) => Ok(Some("the landing will not tick it until the boxes under it are ticked")),
        refused => Err(refused),
    }
}

fn is_trailer(line: &str) -> bool {
    line.starts_with("Item:") || line.starts_with("Box:")
}

pub fn gather_trailers<'a>(messages: &[&'a str]) -> (Vec<String>, Vec<&'a str>) {
    let mut trailers: Vec<&str> = Vec::new();
    let mut item: Option<&str> = None;
    let mut seen: Vec<(Option<&str>, &str)> = Vec::new();
    let bodies = messages
        .iter()
        .map(|message| {
            for line in message.lines().filter(|line| is_trailer(line)) {
                let line = line.trim_end();
                if line.starts_with("Item:") {
                    if item != Some(line) {
                        item = Some(line);
                        trailers.push(line);
                    }
                } else if !seen.contains(&(item, line)) {
                    seen.push((item, line));
                    trailers.push(line);
                }
            }
            message.lines().filter(|line| !is_trailer(line)).collect::<Vec<_>>().join("\n").trim().to_owned()
        })
        .collect();
    (bodies, trailers)
}

pub fn scan(directory: &Path, base: &str, head: &str, github: &Gh, out: &mut impl Write) -> u8 {
    let commits = match tick::commits(directory, base, head) {
        Ok(commits) => commits,
        Err(error) => {
            let _ = writeln!(out, "box scan: {error}");
            return 1;
        }
    };
    let mut bodies: HashMap<String, Result<String, String>> = HashMap::new();
    let mut refused = 0;
    for (sha, message) in commits {
        let short = &sha[..sha.len().min(SHORT_SHA)];
        let trailers = match tick::trailers(&message) {
            Ok(trailers) => trailers,
            Err(error) => {
                refused += 1;
                let _ = writeln!(out, "box scan: {short}: {error}");
                continue;
            }
        };
        for trailer in trailers {
            let issue = match Issue::parse(&trailer.item) {
                Ok(issue) => issue,
                Err(error) => {
                    refused += 1;
                    let _ = writeln!(out, "box scan: {short}: {error}");
                    continue;
                }
            };
            let body = bodies.entry(issue.to_string()).or_insert_with(|| github.body(&issue));
            let line = match body.as_deref().map(|body| verdict(body, &trailer.box_text)) {
                Ok(Ok(None)) => format!("{short}: Box: {} matches an unticked box on {issue}", trailer.box_text),
                Ok(Ok(Some(note))) => format!("{short}: Box: {} matches an unticked box on {issue}; {note}", trailer.box_text),
                Ok(Err(why)) => {
                    refused += 1;
                    format!("{short}: Box: {} matches no unticked box on {issue} ({why}); copy the box text from the issue", trailer.box_text)
                }
                Err(error) => {
                    refused += 1;
                    format!("{short}: cannot read {issue} to check Box: {}: {error}", trailer.box_text)
                }
            };
            let _ = writeln!(out, "box scan: {line}");
        }
    }
    u8::from(refused > 0)
}

pub fn run(directory: &Path, range: &str) -> u8 {
    let Some(range) = super::Range::parse(range) else {
        eprintln!("{}", super::USAGE);
        return 1;
    };
    scan(directory, range.base, range.head, &Gh::new(CALL_LIMIT), &mut std::io::stdout())
}
