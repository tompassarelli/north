//! What is left of the hourly quota the fleet's sessions share on the Socrates Agent App token.
use std::fs;
use std::path::Path;
use std::process::Command;

use super::github_app::{self, Host};

const FRESH_SECONDS: i64 = 60;
const RESERVE: u64 = 1000;

pub struct Pool {
    pub remaining: Option<u64>,
    pub resets_at: i64,
    pub others: Vec<u32>,
}

impl Pool {
    pub fn reserve() -> u64 {
        RESERVE
    }

    pub fn refuses(&self, spent: u32, units: u32) -> bool {
        let Some(left) = self.remaining else {
            return false;
        };
        if left >= RESERVE + u64::from(units) {
            return false;
        }
        left < u64::from(units) || (spent > 0 && u64::from(spent) * self.sessions() >= self.total(spent))
    }

    pub fn average(&self, spent: u32) -> u64 {
        self.total(spent) / self.sessions()
    }

    fn sessions(&self) -> u64 {
        u64::try_from(self.others.len()).unwrap_or(0) + 1
    }

    fn total(&self, spent: u32) -> u64 {
        self.others.iter().map(|used| u64::from(*used)).sum::<u64>() + u64::from(spent)
    }
}

pub fn read(directory: &Path, real_gh: &str, now: i64) -> Option<(u64, i64)> {
    let cache = directory.join(".token-quota");
    if let Some(reading) = fresh(&cache, now) {
        return Some(reading);
    }
    let (token, _) = github_app::agent_token(&Host::from_env(), now, std::env::var("GH_TOKEN").ok()).ok()?;
    let output = Command::new(real_gh).args(["api", "rate_limit"]).env("GH_TOKEN", token).env("GH_PROMPT_DISABLED", "1").output().ok()?;
    let answer: serde_json::Value = serde_json::from_slice(&output.status.success().then_some(output.stdout)?).ok()?;
    let core = &answer["resources"]["core"];
    let (remaining, resets_at) = (core["remaining"].as_u64()?, i64::try_from(core["reset"].as_u64()?).ok()?);
    let _ = fs::create_dir_all(directory).and_then(|()| fs::write(&cache, format!("{now} {remaining} {resets_at}\n")));
    Some((remaining, resets_at))
}

fn fresh(cache: &Path, now: i64) -> Option<(u64, i64)> {
    let text = fs::read_to_string(cache).ok()?;
    let mut fields = text.split_whitespace();
    let (at, remaining, resets_at) = (fields.next()?.parse::<i64>().ok()?, fields.next()?.parse().ok()?, fields.next()?.parse::<i64>().ok()?);
    (now - at < FRESH_SECONDS && resets_at > now).then_some((remaining, resets_at))
}
