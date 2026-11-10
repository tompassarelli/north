use super::super::failure::{DEFERRED, Failure, failed};
use crate::platform::process;
use north_providers::threads::{schema, verb::Failure as LedgerFailure};
use crate::providers::opencode::Shared;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PLAIN_PROMPT: &str = "Reply with the single word ok and nothing else.";
const HEADING_PROMPT: &str = "Reply with the words of any one heading of the standing policy text you were given before this message, without its leading mark, and nothing else.";
const TOKEN_PROMPT: &str = "Reply with the canary token written in the standing policy text you were given before this message, copied exactly, and nothing else.";
const ASK: Duration = Duration::from_secs(30);
const ANSWER_WITHIN: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(3);
const PLACEMENT_BRIEF: &str = "Surface: shell\nNeeds: laptop\n";

pub(in crate::runtime) fn probe(provider: &str) -> Result<(), Failure> {
    let north = std::env::current_exe().map_err(|error| unrun(error.into()))?;
    let scratch = tempfile::tempdir().map_err(|error| unrun(error.into()))?;
    let (state, cwd) = scratch_root(scratch.path())?;
    let (evidence, agents) = evidence(provider, scratch.path()).map_err(unrun)?;
    let scene = Scene { north: &north, state: &state, agents: agents.as_deref() };
    let prompt = match evidence {
        Evidence::Token(_) => TOKEN_PROMPT,
        Evidence::Headings(_) => HEADING_PROMPT,
        Evidence::Nothing => PLAIN_PROMPT,
    };
    let accounts = match provider {
        "claude" => claude_accounts(&north)?,
        _ => vec![None],
    };
    if accounts.is_empty() {
        println!("north runtime canary probe: no Claude account has headroom here, so the launch is not judged");
        return Ok(());
    }
    judge(provider, &accounts, |account, round| {
        let name = format!("canary-{provider}-{}-{round}", std::process::id());
        let attempt = attempt(&scene, provider, &name, &cwd, prompt, account, &evidence);
        let _ = fleet(&scene, &["forget", &name, "--force", "runtime canary probe finished"]);
        attempt
    })
}

/// What the probe's fleet commands run against: the scratch state root, and, for a provider whose policy a planted token travels in, the scratch agent root that `Shared::published` then reads instead of the live generation.
struct Scene<'a> {
    north: &'a Path,
    state: &'a Path,
    agents: Option<&'a Path>,
}

/// What a worker's reply must carry to show the shared policy reached it.
#[derive(Debug, PartialEq)]
pub(super) enum Evidence {
    Token(String),
    Headings(Vec<String>),
    Nothing,
}

/// OpenCode's policy arrives appended to its first turn by `deliver`, so its probe plants a fresh token in a scratch generation holding the published policy and asks for the token back; Claude and Pi read the live profile, which a probe must not touch, so they keep asking for a heading of the published policy.
fn evidence(provider: &str, scratch: &Path) -> Result<(Evidence, Option<PathBuf>), Failure> {
    let published = Shared::published().and_then(|shared| shared.policy());
    if provider != "opencode" {
        let headings: Vec<String> = published.iter().flat_map(|policy| policy.lines()).filter(|line| line.starts_with('#')).map(|line| line.trim_start_matches('#').trim().to_owned()).filter(|heading| !heading.is_empty()).collect();
        return Ok((if headings.is_empty() { Evidence::Nothing } else { Evidence::Headings(headings) }, None));
    }
    let token = fresh_token()?;
    let root = scratch.join("agents");
    let policy = root.join("current/instructions/shared/AGENTS.md");
    std::fs::create_dir_all(policy.parent().unwrap_or(&root))?;
    std::fs::write(&policy, format!("Canary token: {token}\n\n{}\n", published.unwrap_or_default()))?;
    Ok((Evidence::Token(token), Some(root)))
}

fn fresh_token() -> Result<String, Failure> {
    let mut bytes = [0u8; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("north-canary-{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

pub(super) enum Attempt {
    Answered,
    Refused(String),
}

/// Runs the probe on each account in turn until one answers its first turn. A provider that refuses the account itself (a usage limit, a signed-out login, a billing refusal) says nothing of the runtime, so the next account takes the probe, and with none left the probe is not judged rather than failed.
pub(super) fn judge(provider: &str, accounts: &[Option<String>], mut attempt: impl FnMut(Option<&str>, usize) -> Result<Attempt, Failure>) -> Result<(), Failure> {
    for (round, account) in accounts.iter().enumerate() {
        match attempt(account.as_deref(), round + 1)? {
            Attempt::Answered => return Ok(()),
            Attempt::Refused(why) => println!("north runtime canary probe: {provider}{} refused the first turn ({why})", account.as_deref().map_or(String::new(), |account| format!(" on claude:{account}"))),
        }
    }
    println!("north runtime canary probe: every {provider} account refused the first turn, so the launch is not judged");
    Ok(())
}

fn attempt(scene: &Scene, provider: &str, name: &str, cwd: &Path, prompt: &str, account: Option<&str>, evidence: &Evidence) -> Result<Attempt, Failure> {
    match launch(scene, provider, name, cwd, prompt, account) {
        Ok(()) => answer(scene, name, evidence),
        Err(failure) if refused(&failure.message) => Ok(Attempt::Refused(failure.message)),
        Err(failure) => Err(unrun(failure)),
    }
}

/// Whether the provider refused the account, not the runtime: a usage limit, a signed-out or expired login, disabled subscription access, or a billing refusal.
pub(super) fn refused(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let limited = (text.contains("hit your") || text.contains("reached your")) && text.contains("limit");
    limited
        || text.starts_with("429")
        || ["usage limit", "not logged in", "oauth session expired", "disabled claude subscription access", "credit balance", "insufficient credit", "payment required", "billing"].iter().any(|phrase| text.contains(phrase))
}

fn unrun(failure: Failure) -> Failure {
    failed(DEFERRED, format!("the launch never reached a worker, so the probe has no verdict: {}", failure.message))
}

pub(super) fn scratch_root(scratch: &Path) -> Result<(PathBuf, PathBuf), Failure> {
    let state = scratch.join("state");
    let cwd = scratch.join("work");
    std::fs::create_dir_all(&state).and_then(|()| std::fs::create_dir_all(&cwd)).map_err(|error| unrun(error.into()))?;
    schema::ensure(&state.join("threads.db")).map_err(|failure| {
        let why = match failure {
            LedgerFailure::Usage(why) | LedgerFailure::Refused(why) => why,
        };
        unrun(failed(1, format!("the scratch root's ledger, which north fleet opens and widens, could not be created: {why}")))
    })?;
    Ok((state, cwd))
}

fn fleet(scene: &Scene, words: &[&str]) -> Result<String, String> {
    let (state, mut command) = (scene.state, Command::new(scene.north));
    command.arg("fleet").args(words);
    command.env("XDG_STATE_HOME", state).env("NORTH_TEST_ROOT", state).env("THREADS_DB", state.join("threads.db"));
    if let Some(agents) = scene.agents {
        command.env("NORTH_AGENT_STATE_ROOT", agents);
    }
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("NORTH_FLEET_")) {
        command.env_remove(key);
    }
    let output = process::try_run_command_within(command, None, ASK).map_err(|error| error.to_string())?.ok_or_else(|| format!("north fleet {} gave no answer within {}s", words[0], ASK.as_secs()))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        false => Err(format!("north fleet {} failed: {}", words[0], String::from_utf8_lossy(&output.stderr).trim())),
    }
}

fn launch(scene: &Scene, provider: &str, name: &str, cwd: &Path, prompt: &str, account: Option<&str>) -> Result<(), Failure> {
    let cwd = cwd.to_string_lossy();
    let mut words = vec!["launch", "--provider", provider, "--name", name, "--parent", "none", "--test", "--no-wait", "--cwd", &cwd];
    if let Some(account) = account {
        words.extend(["--model", "haiku", "--account", account]);
    }
    words.push(prompt);
    fleet(scene, &words).map(drop).map_err(|why| failed(1, why))
}

fn claude_accounts(north: &Path) -> Result<Vec<Option<String>>, Failure> {
    let brief = tempfile::Builder::new().suffix(".md").tempfile().map_err(|error| unrun(error.into()))?;
    std::fs::write(brief.path(), PLACEMENT_BRIEF).map_err(|error| unrun(error.into()))?;
    let mut command = Command::new(north);
    command.args(["config", "agents", "plan", "--place"]).arg(brief.path());
    let output = process::try_run_command_within(command, None, ASK).map_err(|error| unrun(error.into()))?.ok_or_else(|| unrun(failed(1, "placing a Claude account gave no answer")))?;
    Ok(candidates(&String::from_utf8_lossy(&output.stdout)).into_iter().map(Some).collect())
}

/// The account placement chose, then every other account the plan lists as available, in the plan's order.
pub(super) fn candidates(listing: &str) -> Vec<String> {
    let mut accounts: Vec<String> = placed_account(listing).into_iter().collect();
    if accounts.is_empty() {
        return accounts;
    }
    for label in listing.lines().filter_map(|line| line.trim().strip_prefix("claude:")?.split_once(" available")).map(|(label, _)| label) {
        if !label.contains(':') && !accounts.iter().any(|known| known == label) {
            accounts.push(label.to_owned());
        }
    }
    accounts
}

fn placed_account(listing: &str) -> Option<String> {
    let target = listing.lines().find_map(|line| line.split_once(": target ").map(|(_, rest)| rest))?;
    let mut words = target.split_whitespace();
    let host = words.next()?;
    words.next().and_then(|word| word.strip_prefix("claude:")).filter(|_| !host.ends_with(':')).map(str::to_owned)
}

fn answer(scene: &Scene, name: &str, evidence: &Evidence) -> Result<Attempt, Failure> {
    let started = Instant::now();
    let mut last = "no transcript yet".to_owned();
    while started.elapsed() < ANSWER_WITHIN {
        match fleet(scene, &["show", name, "--json"]) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(session) => match answered(&session) {
                    Some(Turn::Replied(reply) | Turn::Failed(reply)) if refused(&reply) => return Ok(Attempt::Refused(reply)),
                    Some(Turn::Replied(reply)) if !reply.trim().is_empty() => return carries(&reply, evidence).map(|()| Attempt::Answered).map_err(|missing| failed(1, format!("{name} answered without proof of the shared policy: asked for it, it replied {:?} where {missing} was expected", reply.trim()))),
                    Some(Turn::Failed(error)) => return Err(failed(1, format!("{name}'s first turn failed: {error}"))),
                    _ => last = format!("status {}", session["status"].as_str().unwrap_or("unknown")),
                },
                Err(error) => last = format!("fleet show printed invalid JSON: {error}"),
            },
            Err(why) => last = why,
        }
        std::thread::sleep(POLL);
    }
    Err(failed(1, format!("{name} gave no answer within {}s: {last}", ANSWER_WITHIN.as_secs())))
}

pub(super) fn carries(reply: &str, evidence: &Evidence) -> Result<(), String> {
    let reply = folded(reply);
    match evidence {
        Evidence::Token(token) if !reply.contains(&folded(token)) => Err(format!("the token {token}")),
        Evidence::Headings(headings) if !headings.iter().any(|heading| reply.contains(&folded(heading))) => Err(format!("one of the headings {headings:?}")),
        _ => Ok(()),
    }
}

fn folded(text: &str) -> String {
    text.trim().chars().map(|character| match character {
        '\u{2018}' | '\u{2019}' | '\u{201B}' | '\u{2032}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201F}' | '\u{2033}' => '"',
        other => other,
    }).flat_map(char::to_lowercase).collect()
}

enum Turn {
    Replied(String),
    Failed(String),
}

fn answered(session: &Value) -> Option<Turn> {
    let entries = session["entries"].as_array()?;
    if session["status"] == "working" {
        return None;
    }
    let last = entries.iter().rev().find(|entry| matches!(entry["kind"].as_str(), Some("user" | "assistant" | "error")))?;
    let text = last["text"].as_str().unwrap_or_default().to_owned();
    match last["kind"].as_str() {
        Some("assistant") => Some(Turn::Replied(text)),
        Some("error") => Some(Turn::Failed(text)),
        _ => None,
    }
}
