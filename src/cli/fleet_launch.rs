use std::path::PathBuf;

use crate::error::NorthResult;
use crate::fleet::model::{Effort, LaunchRequest, ProviderName};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtime::new_uuid;

use super::fleet_usage::usage;
use super::fleet_lineage;
use super::fleet_placement::{self, Overrides};

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
    overrides: Overrides,
    wait: Option<bool>,
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
            _ if self.overrides.set(flag, &value) => {}
            _ => return Err(usage()),
        }
        Ok(())
    }

    fn request(self, prompt: String, caller: Option<String>, proxy: bool) -> NorthResult<LaunchRequest> {
        let (Some(provider), Some(account)) = (self.provider, self.account) else { return Err(usage()) };
        if prompt.is_empty() {
            return Err(usage());
        }
        let (cwd, machine) = fleet_placement::working_directory(self.cwd)?;
        let name = match self.name {
            Some(name) => name,
            None => default_name(&provider, &account)?,
        };
        let parent = fleet_lineage::parent(self.parent, caller, proxy, &name);
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
            placement: None,
            machine,
            capacity_need: None,
        })
    }
}

#[cfg(test)]
pub(super) fn launch_request(providers: &[ProviderName], words: &[&str], caller: Option<String>) -> NorthResult<LaunchRequest> {
    launch_parts(providers, words, caller, true).map(|(request, _, _)| request)
}

/// The request, its placement overrides and `--wait` (Some(true)) or `--no-wait` (Some(false)).
pub(super) fn launch_parts(providers: &[ProviderName], words: &[&str], caller: Option<String>, proxy: bool) -> NorthResult<(LaunchRequest, Overrides, Option<bool>)> {
    let mut flags = LaunchFlags::default();
    let mut index = 0;
    while let Some(flag) = words.get(index).filter(|word| word.starts_with("--")) {
        if *flag == "--test" {
            flags.verification = true;
            index += 1;
            continue;
        }
        if let wait @ ("--wait" | "--no-wait") = *flag {
            flags.wait = Some(wait == "--wait");
            index += 1;
            continue;
        }
        let value = words.get(index + 1).filter(|value| !value.starts_with("--")).map(|value| (*value).to_owned()).ok_or_else(usage)?;
        flags.set(providers, flag, value)?;
        index += 2;
    }
    let wait = fleet_lineage::wait(flags.wait, flags.parent.as_deref(), caller.as_deref());
    let overrides = std::mem::take(&mut flags.overrides);
    Ok((flags.request(words[index..].join(" "), caller, proxy)?, overrides, wait))
}

fn absolute(path: &str) -> NorthResult<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

fn parse_provider(providers: &[ProviderName], value: &str) -> NorthResult<ProviderName> {
    providers.iter().find(|provider| provider.label() == value).cloned().ok_or_else(usage)
}

fn default_name(provider: &ProviderName, account: &str) -> NorthResult<String> {
    let short: String = new_uuid()?.chars().take(6).collect();
    Ok(format!("{}-{account}-{short}", provider.label()))
}
