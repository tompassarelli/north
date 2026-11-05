use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::digest;
use super::host::{Host, expand_home};

struct Request {
    days: i64,
    noise: Option<String>,
    more: Option<String>,
    feedback: PathBuf,
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut request = Request { days: 7, noise: None, more: None, feedback: expand_home("~/code/south/main/config/news-feedback.jsonl") };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let mut value = || rest.next().cloned().ok_or_else(|| format!("argument {flag}: expected one argument"));
        match flag.as_str() {
            "--days" => request.days = value()?.parse().map_err(|_| "argument --days: invalid int value".to_owned())?,
            "--noise" => request.noise = Some(value()?),
            "--more" => request.more = Some(value()?),
            "--feedback" => request.feedback = value()?.into(),
            other => return Err(format!("unrecognized arguments: {other}")),
        }
    }
    if request.noise.is_some() && request.more.is_some() {
        return Err("argument --more: not allowed with argument --noise".into());
    }
    if request.days <= 0 {
        return Err("days must be positive".into());
    }
    Ok(request)
}

pub(super) fn render(arguments: &[String], host: &dyn Host, path: &Path) -> Result<String, String> {
    let request = parse(arguments)?;
    let wanted = request.noise.clone().or_else(|| request.more.clone());
    let mut items = Vec::new();
    if path.exists() {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| "ledger unreadable".to_owned())?;
        if db.query_row("SELECT 1 FROM sqlite_master WHERE name='watch_digest_items'", [], |_| Ok(())).is_ok() {
            if let Some(identity) = &wanted {
                let label = if request.noise.is_some() { "noise" } else { "more" };
                digest::record_feedback(&db, identity, label, &request.feedback, host.now())?;
                return Ok(format!("Recorded {label}: {identity}\n"));
            }
            items = digest::kept(&db, request.days, host.now())?;
        }
    }
    if wanted.is_some() {
        return Err("no delivered digest item with that ID".into());
    }
    let mut text = String::from("Watch digest: last delivered run\n");
    if items.is_empty() {
        text.push_str("  none\n");
    }
    for item in items {
        text.push_str(&format!("[{}] {}\n{}\n{}\n", item.id, item.title, item.why, item.url));
    }
    Ok(text)
}

pub(super) fn run(arguments: &[String], host: &dyn Host, path: &Path) -> u8 {
    match render(arguments, host, path) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(message) => {
            eprintln!("north model-watch news: error: {message}");
            2
        }
    }
}
