use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, LaunchRequest, ProviderName, Role};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtime::{PROXY, new_uuid};

use super::usage::usage;
use super::lineage;
use super::placement::{self, Overrides};

#[derive(Default)]
struct LaunchFlags {
    verification: bool,
    provider: Option<ProviderName>,
    account: Option<String>,
    cwd: Option<PathBuf>,
    model: Option<String>,
    effort: Option<Effort>,
    name: Option<String>,
    system_prompt: Option<PathBuf>,
    prompt_mode: PromptMode,
    foreground_cap_secs: Option<u64>,
    idle_exit_minutes: Option<u64>,
    parent: Option<String>,
    task_ref: Option<String>,
    claim_box: Option<String>,
    role: Option<Role>,
    overrides: Overrides,
    wait: Option<bool>,
    take_over: bool,
}

impl LaunchFlags {
    fn set(&mut self, providers: &[ProviderName], flag: &str, value: String) -> NorthResult<()> {
        match flag {
            "--provider" => self.provider = Some(parse_provider(providers, &value)?),
            "--account" => self.account = Some(value),
            "--cwd" => self.cwd = Some(PathBuf::from(value)),
            "--model" => self.model = Some(value),
            "--effort" => self.effort = Some(Effort::parse(&value)?),
            "--name" => self.name = Some(value),
            "--system-prompt" => self.system_prompt = Some(absolute(&value)?),
            "--prompt" => self.prompt_mode = PromptMode::parse(&value).ok_or_else(usage)?,
            "--foreground-cap" => self.foreground_cap_secs = Some(value.parse().map_err(|_| usage())?),
            "--idle-exit" => self.idle_exit_minutes = Some(value.parse().map_err(|_| usage())?),
            "--parent" => self.parent = Some(value),
            "--task" => self.task_ref = Some(value),
            "--box" => self.claim_box = Some(value),
            "--role" => self.role = Some(Role::parse(&value)?),
            _ if self.overrides.set(flag, &value) => {}
            _ => return Err(usage()),
        }
        Ok(())
    }

    fn request(self, prompt: String, caller: Option<String>, proxy: bool) -> NorthResult<LaunchRequest> {
        let Some(provider) = self.provider else { return Err(usage()) };
        let placed = provider.label() == "claude" && !self.verification;
        let account = match self.account {
            Some(account) if account.is_empty() => return Err(usage()),
            Some(account) => account,
            None if placed => String::new(),
            None if crate::providers::harness_named(provider.label()).is_some() => crate::providers::opencode::ACCOUNT.to_owned(),
            None => return Err(usage()),
        };
        if prompt.is_empty() {
            return Err(usage());
        }
        let (cwd, machine) = placement::working_directory(self.cwd)?;
        let name = match (self.name, self.role) {
            (Some(name), Some(Role::Proxy)) if name != PROXY => return Err(NorthError::Usage(format!("the proxy is named {PROXY}, not {name}"))),
            (Some(name), _) => name,
            (None, Some(Role::Proxy)) => PROXY.to_owned(),
            (None, _) => default_name(&provider, &account)?,
        };
        let parent = lineage::parent(self.parent, caller, proxy, &name, self.role);
        Ok(LaunchRequest {
            verification: self.verification,
            name,
            provider,
            account,
            cwd,
            model: self.model,
            effort: self.effort,
            prompt,
            system_prompt: self.system_prompt,
            foreground_cap_secs: self.foreground_cap_secs,
            prompt_mode: self.prompt_mode,
            idle_exit_minutes: self.idle_exit_minutes,
            parent,
            task_ref: self.task_ref,
            claim_box: self.claim_box,
            placement: None,
            machine,
            capacity_need: None,
            role: self.role,
            resume: None,
            compact_window: None,
            slots: Default::default(),
        })
    }
}

/// A parsed `north fleet launch`.
pub(super) struct LaunchParts {
    pub request: LaunchRequest,
    pub overrides: Overrides,
    /// `--wait` (Some(true)) or `--no-wait` (Some(false)).
    pub wait: Option<bool>,
    /// `--take-over`: end the session holding the name and give the new one the name and its messages.
    pub take_over: bool,
}

pub(super) fn launch_parts(providers: &[ProviderName], words: &[&str], caller: Option<String>, proxy: bool) -> NorthResult<LaunchParts> {
    let mut flags = LaunchFlags::default();
    let mut index = 0;
    while let Some(flag) = words.get(index).filter(|word| word.starts_with("--")) {
        match *flag {
            "--test" => flags.verification = true,
            "--take-over" => flags.take_over = true,
            "--wait" | "--no-wait" => flags.wait = Some(*flag == "--wait"),
            _ => {
                let value = words.get(index + 1).filter(|value| !value.starts_with("--")).map(|value| (*value).to_owned()).ok_or_else(usage)?;
                flags.set(providers, flag, value)?;
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    let wait = lineage::wait(flags.wait, flags.parent.as_deref(), caller.as_deref());
    let (overrides, take_over) = (std::mem::take(&mut flags.overrides), flags.take_over);
    Ok(LaunchParts { request: flags.request(words[index..].join(" "), caller, proxy)?, overrides, wait, take_over })
}

fn absolute(path: &str) -> NorthResult<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

fn parse_provider(providers: &[ProviderName], value: &str) -> NorthResult<ProviderName> {
    providers.iter().find(|provider| provider.label() == value).cloned().ok_or_else(usage)
}

fn default_name(provider: &ProviderName, account: &str) -> NorthResult<String> {
    let short: String = new_uuid()?.chars().take(6).collect();
    if account.is_empty() {
        return Ok(format!("{}-{short}", provider.label()));
    }
    Ok(format!("{}-{account}-{short}", provider.label()))
}
