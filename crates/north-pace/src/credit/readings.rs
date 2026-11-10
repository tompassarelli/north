//! What the operator read in the provider's console, or an Admin API cost report returned, about an api
//! organization's remaining credit: one file per organization id under `api-readings`, reading
//! `SOURCE USD RFC3339`. A provider reading outranks a console reading until the operator records a newer
//! console figure; between readings north's list-price estimate still stands beside it.
use std::io;
use std::path::{Path, PathBuf};

use crate::foundation::clock::Timestamp;

const CONSOLE: &str = "console";
const PROVIDER: &str = "provider";

/// Where a reading came from: the operator's provider console, or the provider's own cost report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Console,
    Provider,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Console => CONSOLE,
            Self::Provider => PROVIDER,
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            CONSOLE => Some(Self::Console),
            PROVIDER => Some(Self::Provider),
            _ => None,
        }
    }
}

/// A balance the provider showed at `at` (Unix seconds), and where north learned it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub usd: f64,
    pub at: i64,
    pub source: Source,
}

/// The directory the readings are kept under, beside an organization's `api-exhausted` marks.
pub fn directory(state: &Path) -> PathBuf {
    state.join("north/api-readings")
}

/// The reading recorded for organization `id` under `dir`.
pub fn get(dir: &Path, id: &str) -> Option<Reading> {
    let text = std::fs::read_to_string(dir.join(id)).ok()?;
    parse(&text)
}

fn parse(text: &str) -> Option<Reading> {
    let mut words = text.split_whitespace();
    let source = Source::parse(words.next()?)?;
    let usd = words.next()?.parse().ok()?;
    let at = Timestamp::parse(words.next()?).ok()?.unix_seconds();
    Some(Reading { usd, at, source })
}

/// Records `reading` for organization `id` under `dir`; a console reading never replaces a provider reading,
/// so the provider's figure keeps overriding the estimate once it exists. Whether it wrote.
pub fn record(dir: &Path, id: &str, reading: &Reading) -> io::Result<bool> {
    if get(dir, id).is_some_and(|existing| {
        existing.source == Source::Provider && reading.source == Source::Console
    }) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let line = format!(
        "{} {:.2} {}\n",
        reading.source.label(),
        reading.usd,
        Timestamp::from_unix_seconds(reading.at).iso()
    );
    let staged = dir.join(format!(".{id}.{}", std::process::id()));
    std::fs::write(&staged, line)?;
    std::fs::rename(staged, dir.join(id))?;
    Ok(true)
}

/// Records a console reading for `account` (`api:NAME` or `claude:api:NAME`) under the organization id
/// `policy` declares it with, and says what `north fleet accounts --plan` will show.
pub fn record_console(
    policy: &Path,
    state: &Path,
    account: &str,
    usd: f64,
    at: i64,
) -> Result<String, String> {
    if !usd.is_finite() || usd < 0.0 {
        return Err(format!("{usd} is not a credit balance in dollars"));
    }
    let name = account
        .strip_prefix("claude:")
        .unwrap_or(account)
        .strip_prefix("api:")
        .ok_or_else(|| format!("{account} is not an api account; name it api:NAME"))?;
    let orgs = super::orgs(policy);
    let Some((_, id)) = orgs.iter().find(|(org, _)| org == name) else {
        let known: Vec<&str> = orgs.iter().map(|(org, _)| org.as_str()).collect();
        return Err(format!(
            "no api organization named {name}; {} declares {}",
            policy.display(),
            known.join(", ")
        ));
    };
    record(&directory(state), id, &Reading { usd, at, source: Source::Console }).map_err(|error| error.to_string())?;
    Ok(format!(
        "api:{name} reads ${usd:.2} at {}",
        Timestamp::from_unix_seconds(at).iso_minute()
    ))
}

#[cfg(test)]
mod tests;
