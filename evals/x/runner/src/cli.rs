use std::path::{Path, PathBuf};

use crate::error::{Result, RunError};

pub const DEFAULT_SEED: u64 = 20_261_112;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Plan,
    Noise,
    Full,
    Smoke(String),
    Stop,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub command: Command,
    pub account: Option<String>,
    pub token_file: Option<PathBuf>,
    pub policy: PathBuf,
    pub in_flight: usize,
    pub work: PathBuf,
    pub seed: u64,
    pub max_load: f64,
    pub min_five_hour_headroom: f64,
    pub min_week_headroom: f64,
    pub max_reading_age_s: u64,
    pub max_reruns: u32,
}

impl Settings {
    pub fn parse(args: &[String]) -> Result<Self> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.strip_prefix("--") {
                Some(name) => {
                    let value = iter
                        .next()
                        .ok_or_else(|| RunError::Usage(format!("--{name} takes a value")))?;
                    flags.push((name.to_owned(), value.clone()));
                }
                None => positional.push(arg.clone()),
            }
        }
        let command = match positional
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["plan"] => Command::Plan,
            ["noise"] => Command::Noise,
            ["full"] => Command::Full,
            ["smoke", task] => Command::Smoke((*task).to_owned()),
            ["stop"] => Command::Stop,
            _ => return Err(RunError::Usage(USAGE.to_owned())),
        };
        let mut settings = Self {
            command,
            account: None,
            token_file: None,
            policy: std::env::var_os("HOME")
                .map_or_else(PathBuf::new, PathBuf::from)
                .join(".claude/CLAUDE.md"),
            in_flight: 1,
            work: std::env::var_os("EVAL_X_WORK")
                .map_or_else(|| PathBuf::from("/tmp/eval-x"), PathBuf::from),
            seed: DEFAULT_SEED,
            max_load: 2.0,
            min_five_hour_headroom: 20.0,
            min_week_headroom: 10.0,
            max_reading_age_s: 600,
            max_reruns: 2,
        };
        for (name, value) in flags {
            settings.apply(&name, value)?;
        }
        if settings.in_flight == 0 {
            return Err(RunError::Usage("--in-flight must be at least 1".to_owned()));
        }
        Ok(settings)
    }

    fn apply(&mut self, name: &str, value: String) -> Result<()> {
        match name {
            "account" => self.account = Some(value),
            "token-file" => self.token_file = Some(PathBuf::from(value)),
            "policy" => self.policy = PathBuf::from(value),
            "in-flight" => self.in_flight = number(name, &value)?,
            "work" => self.work = PathBuf::from(value),
            "seed" => self.seed = number(name, &value)?,
            "max-load" => self.max_load = number(name, &value)?,
            "min-5h-headroom" => self.min_five_hour_headroom = number(name, &value)?,
            "min-week-headroom" => self.min_week_headroom = number(name, &value)?,
            "max-reading-age" => self.max_reading_age_s = number(name, &value)?,
            "max-reruns" => self.max_reruns = number(name, &value)?,
            _ => return Err(RunError::Usage(format!("unknown option --{name}\n{USAGE}"))),
        }
        Ok(())
    }

    pub fn require_account(&self) -> Result<&str> {
        let account = self
            .account
            .as_deref()
            .ok_or_else(|| RunError::Usage("this command needs --account A".to_owned()))?;
        if account.is_empty() || account.contains(':') {
            return Err(RunError::Refused(format!(
                "account {account} is not a plan account; only a claude plan account letter runs"
            )));
        }
        Ok(account)
    }

    pub fn require_token_file(&self) -> Result<&Path> {
        let file = self
            .token_file
            .as_deref()
            .ok_or_else(|| RunError::Usage("this command needs --token-file FILE".to_owned()))?;
        if std::fs::metadata(file).map_or(0, |found| found.len()) == 0 {
            return Err(RunError::Refused(format!(
                "{} holds no subscription token",
                file.display()
            )));
        }
        Ok(file)
    }
}

fn number<T: std::str::FromStr>(name: &str, value: &str) -> Result<T> {
    value
        .parse()
        .map_err(|_| RunError::Usage(format!("--{name} takes a number, not {value}")))
}

pub const USAGE: &str = "usage: eval-x-runner plan | noise LOGIN | full LOGIN | smoke TASK LOGIN | stop\n  LOGIN: --account A --token-file FILE\n  options: --policy FILE --in-flight N --work DIR --seed N --max-load F --min-5h-headroom P --min-week-headroom P --max-reading-age S --max-reruns N";
