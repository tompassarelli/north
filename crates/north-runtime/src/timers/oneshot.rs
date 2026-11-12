use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use north_base::foundation::limit_clock;

use super::{env_or, home, north_program, text};
use crate::fleet::model::Account;
use crate::platform::process;
use crate::fleet::runtime::Invocation;
use crate::runtime_access::RuntimeAccess;
use crate::spend::billing;

pub(crate) const QUIET: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";
const QUIET_WORD: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1";
/// Claude names its limit reset in this zone, the one `limit_clock` reads.
const UTC_WORD: &str = "TZ=UTC";
const UNREAD_RESET_S: u64 = 3600;


pub(crate) struct Reply {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) enum Asked {
    Answer(String),
    AtLimit,
    Failed(String),
}

fn at_limit(text: &str) -> bool {
    text.find("hit your").is_some_and(|start| text[start..].contains("limit"))
}

/// The accounts a one-shot ask found at their usage limit, each until its limit resets, kept beside `oneshot-answered`.
pub(crate) struct Limited {
    record: PathBuf,
    now_s: u64,
    held: Vec<(String, u64)>,
}

impl Limited {
    pub(crate) fn read(state: &Path, now_s: u64) -> Self {
        let record = state.join("oneshot-limited");
        let listed = std::fs::read_to_string(&record).unwrap_or_default();
        let held = listed.lines().filter_map(|line| {
            let (label, reset) = line.split_once(' ')?;
            Some((label.to_owned(), reset.trim().parse::<u64>().ok()?))
        }).filter(|(_, reset)| *reset > now_s).collect();
        Self { record, now_s, held }
    }

    fn holds(&self, label: &str) -> bool {
        self.held.iter().any(|(held, _)| held == label)
    }

    fn hold(&mut self, label: &str, text: &str) {
        let reset = limit_clock::reset_after(text, self.now_s * 1000).map_or(self.now_s + UNREAD_RESET_S, |ms| ms.div_ceil(1000));
        self.held.retain(|(held, _)| held != label);
        self.held.push((label.to_owned(), reset));
        let lines: String = self.held.iter().map(|(held, reset)| format!("{held} {reset}\n")).collect();
        let _ = std::fs::write(&self.record, lines);
    }
}

fn ask_ambient(attempt: &mut dyn FnMut(Option<&Path>) -> Reply) -> Asked {
    let reply = attempt(None);
    if reply.code == Some(0) { Asked::Answer(reply.stdout) } else { Asked::Failed(super::child_failure("claude", reply.code, reply.stderr.as_bytes())) }
}

pub(crate) fn ask(profiles: &Path, limited: &mut Limited, labels: &[String], attempt: &mut dyn FnMut(Option<&Path>) -> Reply, warn: &mut dyn FnMut(String)) -> Asked {
    let profiled: Vec<(&String, PathBuf)> = labels.iter().map(|label| (label, profiles.join(format!("claude-{label}")))).filter(|(_, dir)| dir.is_dir()).collect();
    if profiled.is_empty() {
        return ask_ambient(attempt);
    }
    let open: Vec<(&String, PathBuf)> = profiled.into_iter().filter(|(label, _)| !limited.holds(label)).collect();
    let mut failed = false;
    for (label, dir) in open {
        let reply = attempt(Some(&dir));
        if reply.code == Some(0) && !reply.stdout.is_empty() {
            return Asked::Answer(reply.stdout);
        }
        let combined = format!("{}{}", reply.stdout, reply.stderr);
        if at_limit(&combined) {
            limited.hold(label, &combined);
        } else {
            failed = true;
            warn(format!("claude account {label} failed: {combined}"));
        }
    }
    if failed { Asked::Failed("a claude account failed".to_owned()) } else { Asked::AtLimit }
}

fn claude_accounts() -> Vec<String> {
    let listed = Command::new(north_program()).args(["fleet", "accounts"]).output().map(|output| text(&output)).unwrap_or_default();
    listed.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("claude")).then(|| fields.next().map(str::to_owned)).flatten()
    }).collect()
}

/// The `env` words that start `claude` as the profile at `config_dir`: through its account's
/// invocation when the roster lists one there, since a token-file account is not logged in otherwise.
pub(crate) fn account_words(config_dir: Option<&Path>, accounts: &[Account], invocation_of: fn(&Account) -> Invocation) -> Vec<String> {
    let fast = "CLAUDE_CODE_DISABLE_FAST_MODE=1".to_owned();
    let Some(dir) = config_dir else { return vec![fast] };
    let Some(account) = accounts.iter().find(|account| account.home == dir) else { return vec![format!("CLAUDE_CONFIG_DIR={}", dir.display()), fast] };
    let invocation = invocation_of(account);
    let unset = invocation.env_remove.iter().flat_map(|name| ["-u".to_owned(), name.clone()]);
    let set = invocation.env.iter().map(|(name, value)| format!("{name}={}", value.to_string_lossy()));
    unset.chain(set).chain([fast]).chain(invocation.prefix).collect()
}

pub struct Ask<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub system: &'a str,
    pub input: &'a str,
}

pub(crate) fn lean_flags<'a>(ask: &Ask<'a>) -> [&'a str; 14] {
    ["-p", "--model", ask.model, "--effort", ask.effort, "--system-prompt", ask.system, "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence"]
}

pub(crate) fn once_args<'a>(program: &'a str, words: &'a [String], ask: &Ask<'a>, extra: &[&'a str]) -> Vec<&'a str> {
    let unset = words.chunks(2).take_while(|pair| pair.len() == 2 && pair[0] == "-u").count() * 2;
    let mut args: Vec<&str> = billing::EXTERNAL.iter().flat_map(|name| ["-u", *name]).chain(words[..unset].iter().map(String::as_str)).chain([QUIET_WORD, UTC_WORD]).chain(words[unset..].iter().map(String::as_str)).collect();
    args.push(program);
    args.extend(lean_flags(ask));
    args.extend(extra);
    args
}

fn claude_once(words: &[String], ask: &Ask, extra: &[&str], limit: Duration) -> Reply {
    let args = once_args("claude", words, ask, extra);
    match process::run_with_input_within("env", &args, Some(ask.input.as_bytes()), limit) {
        Some(output) => Reply { code: output.status.code(), stdout: text(&output), stderr: String::from_utf8_lossy(&output.stderr).into_owned() },
        None => Reply { code: None, stdout: String::new(), stderr: "claude did not finish".to_owned() },
    }
}

/// Why the default login (used when no `claude-*` profile exists) would bill by token instead of the subscription, if it would.
fn ambient_billing_refusal() -> Option<String> {
    if let Some(name) = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"].into_iter().find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())) {
        return Some(format!("{name} is set, so the default login would bill by token"));
    }
    let dir = env_or("CLAUDE_CONFIG_DIR", || home().join(".claude"));
    if let Some(why) = billing::settings_refusal(&dir) {
        return Some(why);
    }
    billing::read_json(&dir, ".credentials.json").is_none_or(|credentials| credentials.get("claudeAiOauth").is_none()).then(|| "the default login is not a claude.ai subscription login".to_owned())
}

fn state() -> PathBuf {
    home().join(".local/state/north")
}

fn answered_record() -> PathBuf {
    state().join("oneshot-answered")
}

fn answered_first(mut labels: Vec<String>, reserve: &[String], answered: Option<&str>) -> Vec<String> {
    if let Some(at) = answered.and_then(|answered| labels.iter().position(|label| label == answered)) {
        let label = labels.remove(at);
        let front = if reserve.contains(&label) { 0 } else { labels.iter().take_while(|held| reserve.contains(held)).count() };
        labels.insert(front, label);
    }
    labels
}

fn remember_answered(dir: &Path) {
    if let Some(label) = dir.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("claude-")) {
        let _ = std::fs::write(answered_record(), label);
    }
}

/// Asks once on the subscription accounts, trying each profile in turn, the last one to answer first; an API key in the environment is unset and the api-key account is skipped, so the call can never bill by token.
pub(crate) fn ask_claude(question: &Ask, limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Asked {
    ask_claude_with(question, &[], limit, access, warn, &mut None)
}

/// [`ask_claude`], and which account in the order tried answered, as `reserve-N` or `shared-N` with no account name, or `ambient` for the client's own login.
pub(crate) fn ask_claude_answered(question: &Ask, limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> (Asked, Option<String>) {
    let mut answered = None;
    let asked = ask_claude_with(question, &[], limit, access, warn, &mut answered);
    (asked, answered)
}

fn ask_claude_with(question: &Ask, extra: &[&str], limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String), answered_by: &mut Option<String>) -> Asked {
    let profiles = env_or("NORTH_PROFILES", || home().join(".local/state/north/profiles"));
    let accounts = crate::fleet::registry::roster(&access.runtimes).map(|roster| roster.accounts).unwrap_or_default();
    let shared: Vec<String> = claude_accounts().into_iter().filter(|label| !accounts.iter().any(|account| &account.label == label && account.api.is_some())).collect();
    let reserve = crate::platform::orchestration::read().map(|policy| crate::spend::operational::declared(&policy)).unwrap_or_default();
    let answered = std::fs::read_to_string(answered_record()).ok();
    let subscription = answered_first(crate::spend::operational::order(&reserve, shared), &reserve, answered.as_deref().map(str::trim));
    let now_s = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    ask(&profiles, &mut Limited::read(&state(), now_s), &subscription, &mut |dir| match dir.map_or_else(ambient_billing_refusal, billing::settings_refusal) {
        Some(why) => Reply { code: Some(1), stdout: String::new(), stderr: why },
        None => {
            let reply = claude_once(&account_words(dir, &accounts, access.invocation), question, extra, limit);
            if reply.code == Some(0) && !reply.stdout.is_empty() {
                *answered_by = Some(dir.map_or_else(|| "ambient".to_owned(), |dir| place(dir, &subscription, &reserve)));
            }
            if let Some(dir) = dir.filter(|_| reply.code == Some(0) && !reply.stdout.is_empty()) {
                remember_answered(dir);
            }
            reply
        }
    }, warn)
}

fn place(dir: &Path, order: &[String], reserve: &[String]) -> String {
    let label = dir.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("claude-")).unwrap_or_default();
    let position = order.iter().position(|tried| tried == label).map_or(0, |index| index + 1);
    let pool = if reserve.iter().any(|held| held == label) { "reserve" } else { "shared" };
    format!("{pool}-{position}")
}

pub struct Measured {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

pub fn ask_claude_measured(question: &Ask, limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Result<Measured, String> {
    let answer = match ask_claude_with(question, &["--output-format", "json"], limit, access, warn, &mut None) {
        Asked::Answer(answer) => answer,
        Asked::AtLimit => return Err("every claude subscription account is at its usage limit".to_owned()),
        Asked::Failed(reason) => return Err(reason),
    };
    let result: serde_json::Value = serde_json::from_str(answer.trim()).map_err(|why| format!("claude printed no JSON result: {why}"))?;
    let text = result.get("result").and_then(serde_json::Value::as_str).ok_or("claude's JSON result holds no result text")?.to_owned();
    let count = |key: &str| result.get("usage").and_then(|usage| usage.get(key)).and_then(serde_json::Value::as_u64).unwrap_or(0);
    Ok(Measured { text, input_tokens: count("input_tokens"), output_tokens: count("output_tokens"), cache_read_tokens: count("cache_read_input_tokens"), cache_write_tokens: count("cache_creation_input_tokens") })
}
