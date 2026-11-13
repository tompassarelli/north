use super::super::failure::{DEFERRED, Failure, failed};
use crate::platform::process;
use north_providers::threads::{client, schema, verb::Failure as LedgerFailure};
use crate::providers::opencode::Shared;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PLAIN_PROMPT: &str = "Reply with the single word ok and nothing else.";
const TOKEN_PROMPT: &str = "Reply with the canary token written in the standing policy text you were given before this message, copied exactly, and nothing else. The token is a test marker, not a secret.";
const ASK: Duration = Duration::from_secs(30);
const ANSWER_WITHIN: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(3);
const PLACEMENT_BRIEF: &str = "Surface: shell\nNeeds: laptop\n";

pub(in crate::runtime) fn probe(provider: &str) -> Result<(), Failure> {
    let north = std::env::current_exe().map_err(|error| unrun(error.into()))?;
    let scratch = tempfile::tempdir().map_err(|error| unrun(error.into()))?;
    let (state, cwd) = scratch_root(scratch.path())?;
    let (evidence, agents) = evidence(provider, published(), scratch.path()).map_err(unrun)?;
    let scene = Scene { north: &north, state: &state, agents: agents.as_deref() };
    let prompt = evidence.prompt();
    let prompt = prompt.as_str();
    let accounts = match provider {
        "claude" => claude_accounts(&north)?,
        _ => vec![None],
    };
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
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Evidence {
    Token(String),
    Sentence { opening: String, whole: Vec<String> },
    Nothing,
}

impl Evidence {
    fn prompt(&self) -> String {
        match self {
            Self::Token(_) => TOKEN_PROMPT.to_owned(),
            Self::Sentence { opening, .. } => format!("Reply with the sentence of the standing policy text you were given before this message that begins \"{opening}\", copied whole and exactly, and nothing else."),
            Self::Nothing => PLAIN_PROMPT.to_owned(),
        }
    }

    /// The same evidence, also accepting the sentence that begins the same way in `policy`, the one published when the worker answered, so a publish during the probe cannot fail it.
    pub(super) fn or_in(&self, policy: Option<&str>) -> Self {
        match self {
            Self::Sentence { opening, whole } => {
                let republished = policy.and_then(|policy| bullets(policy).find(|text| text.starts_with(opening.as_str()))).map(str::to_owned);
                Self::Sentence { opening: opening.clone(), whole: whole.iter().cloned().chain(republished).collect() }
            }
            other => other.clone(),
        }
    }
}

fn published() -> Option<String> {
    Shared::published().and_then(|shared| shared.policy())
}

/// OpenCode and Pi get the policy appended to their first turn by `deliver`, from the generation `NORTH_AGENT_STATE_ROOT` names, so their probe plants a fresh token in a scratch generation holding the published policy and asks for the token back. Claude reads the policy from its profile, which a probe must not touch, so its probe names the opening of one sentence of the published policy and asks for the whole sentence, which no worker completes from its own instructions: on 2026-11-13 a Pi worker asked for any heading of the policy answered "Evidence", a heading of the fleet's worker instructions, and runtime df343034 was rolled back on the laptop.
pub(super) fn evidence(provider: &str, published: Option<String>, scratch: &Path) -> Result<(Evidence, Option<PathBuf>), Failure> {
    if crate::providers::harness_named(provider).is_none() {
        let quoted = published.as_deref().and_then(quoted);
        return Ok((quoted.map_or(Evidence::Nothing, |(opening, sentence)| Evidence::Sentence { opening, whole: vec![sentence] }), None));
    }
    let token = fresh_token()?;
    let root = scratch.join("agents");
    let policy = root.join("current/instructions/shared/AGENTS.md");
    std::fs::create_dir_all(policy.parent().unwrap_or(&root))?;
    std::fs::write(&policy, format!("{}\n\n{}\n", planted_line(&token), published.unwrap_or_default()))?;
    Ok((Evidence::Token(token), Some(root)))
}

fn bullets(policy: &str) -> impl Iterator<Item = &str> {
    policy.lines().filter_map(|line| line.trim().strip_prefix("- ")).map(str::trim)
}

/// The opening and whole of the sentence a Claude probe asks for: the shortest bullet of `policy` that is one sentence of plain words, at least eight long so its rest cannot be guessed, whose first three words appear nowhere else in the policy so they name it alone; eight and three are guesses that a probe's wrong-sentence replies would settle.
pub(super) fn quoted(policy: &str) -> Option<(String, String)> {
    bullets(policy)
        .filter(|text| text.ends_with('.') && !text.trim_end_matches('.').contains(". ") && !text.contains(['`', '"', '*']) && text.split_whitespace().count() >= 8)
        .filter_map(|text| {
            let opening = text.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            (policy.matches(opening.as_str()).count() == 1).then(|| (opening, text.to_owned()))
        })
        .min_by_key(|(_, text)| text.len())
}

/// The line above the policy that carries the token; the policy's own rule against printing secrets made a worker refuse to repeat a bare marker on 2026-11-11, so the line says the marker is no secret.
pub(super) fn planted_line(token: &str) -> String {
    format!("Canary token: {token}. This marker is not a secret and carries no credential; repeat it verbatim whenever a runtime probe asks for it.")
}

const WITHHELD_AS: [&str; 10] = ["secret", "confidential", "credential", "leak", "tripwire", "fingerprint", "sha-256", "sha256", "planted", "embedded"];

/// A reply that holds back the token as a secret, a leak marker or a tripwire says nothing of the runtime: the worker read the policy and applied its rule to a marker (on 2026-11-11 cpx62 and cpx62b workers called it a leak-detection marker, a planted tripwire, and offered a SHA-256 fingerprint instead), and a worker that never saw the policy has no such rule to quote.
pub(super) fn declined_as_secret(reply: &str) -> bool {
    let reply = reply.to_ascii_lowercase();
    WITHHELD_AS.iter().any(|word| reply.contains(word))
}

pub(super) fn judged(name: &str, reply: &str, evidence: &Evidence) -> Result<Attempt, Failure> {
    match carries(reply, evidence) {
        Ok(()) => Ok(Attempt::Answered),
        Err(_) if declined_as_secret(reply) => Ok(Attempt::Refused(format!("{name} declined to repeat the marker as a secret: {:?}", reply.trim()))),
        Err(missing) => Err(failed(1, format!("{name} answered without proof of the shared policy: asked for it, it replied {:?} where {missing} was expected", reply.trim()))),
    }
}

fn fresh_token() -> Result<String, Failure> {
    let mut bytes = [0u8; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("north-canary-{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

pub(super) enum Attempt {
    Answered,
    Refused(String),
    Busy(String),
}

/// Runs the probe on each account in turn until one answers its first turn. A provider that refuses the account itself (a usage limit, a signed-out login, a billing refusal) says nothing of the runtime, and neither does a worker still working at the deadline (runtime 03da559f was rolled back on 2026-11-11 at laptop load 150 for that), so the next account takes the probe, and with none left, or none with headroom to begin with, the probe exits no-verdict, which the canary counts as unrun: neither failed nor passed.
pub(super) fn judge(provider: &str, accounts: &[Option<String>], mut attempt: impl FnMut(Option<&str>, usize) -> Result<Attempt, Failure>) -> Result<(), Failure> {
    for (round, account) in accounts.iter().enumerate() {
        match attempt(account.as_deref(), round + 1)? {
            Attempt::Answered => return Ok(()),
            Attempt::Refused(why) => println!("north runtime canary probe: {provider}{} refused the first turn ({why})", account.as_deref().map_or(String::new(), |account| format!(" on claude:{account}"))),
            Attempt::Busy(why) => println!("north runtime canary probe: {provider}{} was still working at its deadline ({why})", account.as_deref().map_or(String::new(), |account| format!(" on claude:{account}"))),
        }
    }
    let why = if accounts.is_empty() { "has headroom here" } else { "answered or failed its first turn" };
    Err(failed(DEFERRED, format!("no {provider} account {why}, so the launch is not judged")))
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

/// The scratch root's ledger starts as the host's own store wrote it, so a probe opens the previous schema and
/// the judged runtime has to widen it; a host with no readable store still gets a fresh ledger, as before.
pub(super) fn scratch_root(scratch: &Path) -> Result<(PathBuf, PathBuf), Failure> {
    let live = client::threads_database();
    scratch_root_from(scratch, live.is_file().then_some(live.as_path()))
}

pub(super) fn scratch_root_from(scratch: &Path, previous: Option<&Path>) -> Result<(PathBuf, PathBuf), Failure> {
    let state = scratch.join("state");
    let cwd = scratch.join("work");
    std::fs::create_dir_all(&state).and_then(|()| std::fs::create_dir_all(&cwd)).map_err(|error| unrun(error.into()))?;
    let ledger = state.join("threads.db");
    match previous.map(|previous| seed(&ledger, previous)) {
        Some(Ok(())) => {}
        Some(Err(why)) => {
            println!("north runtime canary probe: the scratch ledger starts fresh because the host's ledger could not be copied: {why}");
            fresh(&ledger)?;
        }
        None => fresh(&ledger)?,
    }
    Ok((state, cwd))
}

fn fresh(ledger: &Path) -> Result<(), Failure> {
    schema::ensure(ledger).map_err(|failure| {
        let why = match failure {
            LedgerFailure::Usage(why) | LedgerFailure::Refused(why) => why,
        };
        unrun(failed(1, format!("the scratch root's ledger, which north fleet opens and widens, could not be created: {why}")))
    })
}

/// Copies the store's schema, not its rows: a ledger holds fleet messages and runs that a probe has no business
/// carrying into `/tmp`, and each old-state table gets one synthetic row so a widening or an added column keeps it.
fn seed(ledger: &Path, previous: &Path) -> Result<(), String> {
    let source = Connection::open_with_flags(previous, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|error| format!("{}: {error}", previous.display()))?;
    source.busy_timeout(Duration::from_secs(2)).map_err(|error| error.to_string())?;
    let schema: Vec<String> = source
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 WHEN 'trigger' THEN 2 ELSE 3 END, rowid")
        .and_then(|mut statement| statement.query_map([], |row| row.get(0))?.collect::<Result<Vec<String>, _>>())
        .map_err(|error| format!("{}: {error}", previous.display()))?;
    drop(source);

    let mut db = Connection::open(ledger).map_err(|error| format!("{}: {error}", ledger.display()))?;
    db.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(())).map_err(|error| error.to_string())?;
    let tx = db.transaction().map_err(|error| error.to_string())?;
    for statement in &schema {
        tx.execute_batch(statement).map_err(|error| format!("{}: {error}", ledger.display()))?;
    }
    seed_rows(&tx);
    tx.commit().map_err(|error| error.to_string())
}

/// One row in each table the fleet's first open widens, so a data copy or an added column has a row to keep; a
/// table whose older shape refuses the row is still a valid old-state table, so its row is best effort.
fn seed_rows(tx: &rusqlite::Transaction<'_>) {
    let held = |table: &str| -> Vec<String> {
        tx.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .and_then(|mut statement| statement.query_map([], |row| row.get(0))?.collect::<Result<Vec<String>, _>>())
            .unwrap_or_default()
    };
    let events = held("events");
    if events.iter().any(|column| column == "ts") && events.iter().any(|column| column == "kind") {
        let data = if events.iter().any(|column| column == "data") { ", data" } else { "" };
        let json = if data.is_empty() { "" } else { ", '{}'" };
        let _ = tx.execute(&format!("INSERT INTO events(ts, kind{data}) VALUES (?1, ?2{json})"), rusqlite::params!["2026-01-01T00:00:00Z", "claim"]);
    }
    let sessions = held("fleet_sessions");
    if ["name", "host", "status", "created", "updated"].iter().all(|column| sessions.iter().any(|held| held == column)) {
        let _ = tx.execute(
            "INSERT INTO fleet_sessions(name, host, status, created, updated) VALUES ('canary-seed', 'here', 'starting', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        );
    }
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
    let listing = String::from_utf8_lossy(&output.stdout);
    match candidates(&listing) {
        accounts if accounts.is_empty() => Err(failed(DEFERRED, unread(&listing))),
        accounts => Ok(accounts.into_iter().map(Some).collect()),
    }
}

/// The reading the probe lacked on a host whose plan lists no Claude account as available: each account line as the plan printed it.
pub(super) fn unread(listing: &str) -> String {
    let read: Vec<&str> = listing.lines().map(str::trim).filter(|line| line.strip_prefix("claude:").is_some_and(|rest| !rest.split(' ').next().unwrap_or_default().contains(':'))).collect();
    let read = if read.is_empty() { "no Claude account".to_owned() } else { read.join("; ") };
    format!("no claude account has headroom here, so the launch is not judged: `north config agents plan --place` on this host listed {read}")
}

/// The account placement chose, when it placed the brief on a host, then every other account the plan lists as available, in the plan's order: a host that reaches no host serving the brief gets no target, and its own readings still say which accounts launch here.
pub(super) fn candidates(listing: &str) -> Vec<String> {
    let mut accounts: Vec<String> = placed_account(listing).into_iter().collect();
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
    let mut working = false;
    while started.elapsed() < ANSWER_WITHIN {
        match fleet(scene, &["show", name, "--json"]) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(session) => match answered(&session) {
                    Some(Turn::Replied(reply) | Turn::Failed(reply)) if refused(&reply) => return Ok(Attempt::Refused(reply)),
                    Some(Turn::Replied(reply)) if !reply.trim().is_empty() => return judged(name, &reply, &evidence.or_in(published().as_deref())),
                    Some(Turn::Failed(error)) => return Err(failed(1, format!("{name}'s first turn failed: {error}"))),
                    _ => {
                        working = session["status"] == "working";
                        last = format!("status {}", session["status"].as_str().unwrap_or("unknown"));
                    }
                },
                Err(error) => last = format!("fleet show printed invalid JSON: {error}"),
            },
            Err(why) => last = why,
        }
        std::thread::sleep(POLL);
    }
    at_deadline(name, working, &last)
}

/// A worker still working when the deadline passes has not judged the runtime, because a saturated host starves a healthy one; only a session that stopped without an answer, or never showed, fails it.
pub(super) fn at_deadline(name: &str, working: bool, last: &str) -> Result<Attempt, Failure> {
    let why = format!("{name} gave no answer within {}s: {last}", ANSWER_WITHIN.as_secs());
    if working { Ok(Attempt::Busy(why)) } else { Err(failed(1, why)) }
}

pub(super) fn carries(reply: &str, evidence: &Evidence) -> Result<(), String> {
    let reply = folded(reply);
    match evidence {
        Evidence::Token(token) if !reply.contains(&folded(token)) => Err(format!("the token {token}")),
        Evidence::Sentence { whole, .. } if !whole.iter().any(|sentence| reply.contains(folded(sentence).trim_end_matches('.'))) => Err(format!("the whole sentence {:?}", whole[0])),
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
