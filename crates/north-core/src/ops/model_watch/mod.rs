mod alerts;
mod digest;
mod geometry;
mod host;
mod html;
mod models;
mod news;
mod rank;
mod run;
mod sources;
#[cfg(test)]
mod tests;
mod xml;

use std::env;
use std::path::PathBuf;

use serde_json::Value;

use self::host::{Host, Real, expand_home, preferred_profile};

const SOURCES: &str = include_str!("../../../../../harness/model-watch-sources.toml");
const USAGE: &str = "usage: north model-watch [--interests PATH] [--feedback PATH] [--topic TOPIC] [--alerts-only] [--location PATH] [--sources PATH] [--timeout SECONDS] [--proposals DIR]
       north model-watch news [--days N] [--noise ID | --more ID] [--feedback PATH]";

pub(super) struct Options {
    pub(super) interests: PathBuf,
    pub(super) feedback: PathBuf,
    pub(super) topic: String,
    pub(super) alerts_only: bool,
    pub(super) location: PathBuf,
    pub(super) sources: Option<PathBuf>,
    pub(super) timeout: f64,
    pub(super) proposals: PathBuf,
    pub(super) db: PathBuf,
    pub(super) usage_db: PathBuf,
    pub(super) ntfy_url: Option<String>,
    pub(super) ntfy_topic: String,
    pub(super) ntfy_token_file: Option<PathBuf>,
    pub(super) external_billing: bool,
    pub(super) north: String,
}

pub(super) fn state_db() -> PathBuf {
    env::var_os("THREADS_DB").map_or_else(|| expand_home("~/.local/state/threads/threads.db"), PathBuf::from)
}

fn present(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

impl Options {
    pub(super) fn defaults() -> Self {
        let remote = present("THREADS_DB").is_none() && expand_home("~/.config/threads/remote").exists();
        Self {
            interests: expand_home("~/code/south/main/config/interests.md"),
            feedback: expand_home("~/code/south/main/config/news-feedback.jsonl"),
            topic: "normal".into(),
            alerts_only: false,
            location: expand_home("~/code/south/main/config/location.toml"),
            sources: None,
            timeout: 20.0,
            proposals: expand_home("~/.local/state/agents/model-watch/proposals"),
            db: state_db(),
            usage_db: if remote { expand_home("~/.local/state/threads/snapshot.db") } else { state_db() },
            ntfy_url: present("NTFY_URL"),
            ntfy_topic: present("NTFY_URGENT_TOPIC").unwrap_or_else(|| "urgent".into()),
            ntfy_token_file: present("NTFY_TOKEN_FILE").map(PathBuf::from),
            external_billing: ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY"].iter().any(|name| present(name).is_some()),
            north: env::current_exe().map_or_else(|_| "north".into(), |path| path.to_string_lossy().into_owned()),
        }
    }

    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self::defaults();
        let mut rest = arguments.iter();
        while let Some(flag) = rest.next() {
            let mut value = || rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--alerts-only" => options.alerts_only = true,
                "--interests" => options.interests = value()?.into(),
                "--feedback" => options.feedback = value()?.into(),
                "--topic" => options.topic = value()?,
                "--location" => options.location = value()?.into(),
                "--sources" => options.sources = Some(value()?.into()),
                "--proposals" => options.proposals = value()?.into(),
                "--timeout" => options.timeout = value()?.parse().map_err(|_| "timeout must be a number".to_owned())?,
                other => return Err(format!("unrecognized argument {other}")),
            }
        }
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<(), String> {
        if !(self.timeout > 0.0 && self.timeout <= 60.0) {
            return Err("timeout must be greater than 0 and at most 60 seconds".into());
        }
        if self.topic.is_empty() || !self.topic.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            return Err("invalid digest topic".into());
        }
        Ok(())
    }
}

pub(super) fn toml_json(item: &toml_edit::Item) -> Value {
    if let Some(table) = item.as_table_like() {
        return Value::Object(table.iter().map(|(key, value)| (key.to_owned(), toml_json(value))).collect());
    }
    if let Some(tables) = item.as_array_of_tables() {
        return Value::Array(tables.iter().map(|table| toml_json(&toml_edit::Item::Table(table.clone()))).collect());
    }
    match item.as_value() {
        Some(toml_edit::Value::String(text)) => text.value().clone().into(),
        Some(toml_edit::Value::Integer(number)) => (*number.value()).into(),
        Some(toml_edit::Value::Float(number)) => serde_json::Number::from_f64(*number.value()).map_or(Value::Null, Value::Number),
        Some(toml_edit::Value::Boolean(flag)) => (*flag.value()).into(),
        Some(toml_edit::Value::Array(items)) => Value::Array(items.iter().map(|value| toml_json(&toml_edit::Item::Value(value.clone()))).collect()),
        _ => Value::Null,
    }
}

pub(super) fn load_sources(options: &Options) -> Result<Value, String> {
    let text = match &options.sources {
        Some(path) => std::fs::read_to_string(path).map_err(|error| error.to_string())?,
        None => SOURCES.to_owned(),
    };
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| error.to_string())?;
    Ok(toml_json(document.as_item()))
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.first().map(String::as_str) == Some("news") {
        return news::run(&arguments[1..], &Real { config_dir: None }, &state_db());
    }
    let options = match Options::parse(arguments) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("north model-watch: {error}\n{USAGE}");
            return 2;
        }
    };
    execute(&Real { config_dir: claude_config_dir(&options) }, &options)
}

fn claude_config_dir(options: &Options) -> Option<PathBuf> {
    if let Some(dir) = present("CLAUDE_CONFIG_DIR") {
        return Some(dir.into());
    }
    let listed = std::process::Command::new(&options.north).args(["fleet", "accounts"]).output().ok()?;
    let profiles = present("NORTH_PROFILES").map_or_else(|| expand_home("~/.local/state/north/profiles"), PathBuf::from);
    preferred_profile(&String::from_utf8_lossy(&listed.stdout), &profiles)
}

fn execute(host: &dyn Host, options: &Options) -> u8 {
    let config = match load_sources(options) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("north model-watch: sources: {error}");
            return 1;
        }
    };
    if options.alerts_only {
        return alerts::run(host, options, &config);
    }
    run::watch(host, options, &config)
}
