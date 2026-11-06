use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::step::Step;
use north_base::platform::process::exit_status;
use north_base::platform::push_ledger;

use super::git::{quiet, read};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hooks {
    Run,
    Skip,
}

pub struct Push<'a> {
    pub source: Option<&'a Path>,
    pub url: &'a str,
    pub refspec: &'a str,
    pub lease: Option<&'a str>,
    pub hooks: Hooks,
    pub claimed: &'a str,
}

pub trait Origin {
    fn snapshot(&self, url: &str, destination_ref: &str) -> Result<Snapshot, String>;
    fn symref(&self, url: &str, reference: &str) -> Option<String>;
    fn head(&self, url: &str) -> Option<String>;
    fn tip(&self, url: &str, reference: &str) -> Option<String>;
    fn listing(&self, url: &str, references: &[&str]) -> Step<String>;
    fn fetch(&self, refspec: &str) -> Step;
    fn fetch_scratch(&self, url: &str, refspec: &str) -> bool;
    fn push(&self, push: &Push) -> Step;
}

pub struct Live {
    directory: PathBuf,
}

impl Live {
    pub fn new(directory: &Path) -> Self {
        Self { directory: directory.to_path_buf() }
    }

    fn first_oid(&self, url: &str, reference: &str) -> Option<String> {
        let listing = quiet(&self.directory, &["ls-remote", url, reference])?;
        listing.lines().next().and_then(|line| line.split('\t').next()).map(str::to_owned)
    }
}

impl Origin for Live {
    fn snapshot(&self, url: &str, destination_ref: &str) -> Result<Snapshot, String> {
        let listing = self.symref(url, destination_ref).ok_or_else(|| format!("safe-push: cannot read origin destination {destination_ref}\n"))?;
        snapshot(&listing, destination_ref)
    }

    fn symref(&self, url: &str, reference: &str) -> Option<String> {
        quiet(&self.directory, &["ls-remote", "--symref", url, reference])
    }

    fn head(&self, url: &str) -> Option<String> {
        self.first_oid(url, "HEAD")
    }

    fn tip(&self, url: &str, reference: &str) -> Option<String> {
        self.first_oid(url, reference)
    }

    fn listing(&self, url: &str, references: &[&str]) -> Step<String> {
        let mut arguments = vec!["ls-remote", url];
        arguments.extend(references);
        read(&self.directory, &arguments)
    }

    fn fetch(&self, refspec: &str) -> Step {
        read(&self.directory, &["fetch", "-q", "--no-tags", "origin", refspec]).map(drop)
    }

    fn fetch_scratch(&self, url: &str, refspec: &str) -> bool {
        quiet(&self.directory, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", url, refspec]).is_some()
    }

    fn push(&self, push: &Push) -> Step {
        exit_status(push_command(push, &self.directory).status())
    }
}

pub fn repository(url: &str) -> String {
    push_ledger::repository(url)
}

pub fn push_command(push: &Push, directory: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(push.source.unwrap_or(directory)).env("SAFE_PUSH_ACTIVE", "1").env("SAFE_PUSH_OUTPUT_CLAIMED", push.claimed).args(["push", "--no-follow-tags"]);
    if push.hooks == Hooks::Skip {
        command.arg("--no-verify");
    }
    command.args(push.lease).args([push.url, push.refspec]);
    command
}

#[derive(Debug, Eq, PartialEq)]
pub enum Snapshot {
    Absent,
    Object(String),
}

impl Snapshot {
    pub fn render(&self) -> String {
        match self {
            Self::Absent => "absent".to_owned(),
            Self::Object(oid) => format!("object:{oid}"),
        }
    }
}

pub fn snapshot(observed: &str, destination_ref: &str) -> Result<Snapshot, String> {
    if observed.is_empty() {
        return Ok(Snapshot::Absent);
    }
    let refused = |what: &str| format!("safe-push: origin destination {destination_ref} is {what} — refusing\n");
    let mut found = Vec::new();
    for line in observed.split('\n') {
        let fields: Vec<&str> = line.split('\t').filter(|field| !field.is_empty()).collect();
        let value = fields.first().copied().unwrap_or("");
        if value.starts_with("ref: ") {
            return Err(refused("symbolic"));
        }
        if !object_id(value) || fields.get(1) != Some(&destination_ref) || fields.len() > 2 {
            return Err(refused("malformed or ambiguous"));
        }
        found.push(value);
    }
    match found.as_slice() {
        [oid] => Ok(Snapshot::Object((*oid).to_owned())),
        _ => Err(refused("malformed or ambiguous")),
    }
}

fn object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
#[path = "origin_tests.rs"]
mod tests;
