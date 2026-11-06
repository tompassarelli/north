use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use time::OffsetDateTime;

pub(super) const ENROLLED: [&str; 5] = ["north", "makgora", "wisp", "padbridge", "makgora-client"];
const LAUNCH_GRACE_SECONDS: i64 = 60 * 60;
static BRIEF: std::sync::LazyLock<&str> = std::sync::LazyLock::new(|| north_base::platform::harness::table("harness/code-health/review-brief.md"));

pub(super) struct TickEnv {
    pub code_root: PathBuf,
    pub state_root: PathBuf,
    pub handoffs: PathBuf,
    pub code_health: Vec<String>,
    pub north: String,
    pub dry_run: bool,
    pub now: OffsetDateTime,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Verdict {
    Quiet,
    Held,
    Deferred,
    Started,
    Missing,
    Error,
}

impl Verdict {
    const fn word(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Held => "held",
            Self::Deferred => "deferred",
            Self::Started => "started",
            Self::Missing => "missing",
            Self::Error => "error",
        }
    }
}

pub(super) struct Outcome {
    pub verdict: Verdict,
    pub detail: String,
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

pub(super) fn review_name(repo: &str, at: OffsetDateTime) -> String {
    format!("code-health-review-{repo}-{:04}{:02}{:02}-{:02}{:02}", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute())
}

fn iso(at: OffsetDateTime) -> String {
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second(), at.millisecond())
}

fn run(argv: &[String], cwd: &Path) -> Ran {
    let Some((program, rest)) = argv.split_first() else {
        return Ran { code: 127, stdout: String::new(), stderr: "empty command".to_owned() };
    };
    match Command::new(program).args(rest).current_dir(cwd).stdin(Stdio::null()).output() {
        Ok(output) => Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => Ran { code: 127, stdout: String::new(), stderr: format!("{program}: {error}") },
    }
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|&word| word.to_owned()).collect()
}

fn fleet_names(env: &TickEnv) -> Option<HashSet<String>> {
    let listed = run(&[env.north.clone(), "fleet".to_owned(), "list".to_owned(), "--json".to_owned()], &env.code_root);
    if listed.code != 0 {
        return None;
    }
    let sessions: Vec<serde_json::Value> = serde_json::from_str(&listed.stdout).ok()?;
    Some(sessions.iter().filter_map(|session| session.get("name")?.as_str().map(str::to_owned)).collect())
}

fn locked_name(lock: &Path) -> String {
    fs::read_to_string(lock)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("name").map(|name| name.as_str().map_or_else(|| name.to_string(), str::to_owned)))
        .unwrap_or_default()
}

fn lock_age_seconds(lock: &Path, now: OffsetDateTime) -> Option<i64> {
    let modified = fs::metadata(lock).ok()?.modified().ok()?;
    Some((now - OffsetDateTime::from(modified)).whole_seconds())
}

fn acquire(lock: &Path, name: &str, env: &TickEnv) -> Result<bool, String> {
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    for _ in 0..2 {
        match fs::OpenOptions::new().write(true).create_new(true).open(lock) {
            Ok(mut file) => {
                let record = serde_json::json!({ "name": name, "at": iso(env.now), "pid": std::process::id() });
                writeln!(file, "{record}").map_err(|error| format!("{}: {error}", lock.display()))?;
                return Ok(true);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", lock.display())),
        }
        let Some(names) = fleet_names(env) else { return Ok(false) };
        let young = lock_age_seconds(lock, env.now).is_none_or(|age| age < LAUNCH_GRACE_SECONDS);
        if names.contains(&locked_name(lock)) || young {
            return Ok(false);
        }
        let _ = fs::remove_file(lock);
    }
    Ok(false)
}

fn fill(template: &str, values: &[(&str, String)]) -> String {
    let pattern = regex::Regex::new(r"\{\{(\w+)\}\}").expect("static pattern");
    pattern
        .replace_all(template, |captures: &regex::Captures<'_>| {
            values.iter().find(|(key, _)| *key == &captures[1]).map_or_else(|| captures[0].to_owned(), |(_, value)| value.clone())
        })
        .into_owned()
}

fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

fn checkout_of(code_root: &Path, repo: &str) -> PathBuf {
    code_root.join(if repo == "firn" { "nixos-config" } else { repo }).join("main")
}

pub(super) fn tick_repo(repo: &str, env: &TickEnv, print: &mut dyn FnMut(String)) -> Outcome {
    let checkout = checkout_of(&env.code_root, repo);
    if !checkout.exists() {
        return Outcome { verdict: Verdict::Missing, detail: format!("no checkout at {}", checkout.display()) };
    }
    let name = review_name(repo, env.now);
    let lock = env.state_root.join(repo).join("review.lock");
    match acquire(&lock, &name, env) {
        Ok(true) => {}
        Ok(false) => return Outcome { verdict: Verdict::Held, detail: format!("review lock {}", lock.display()) },
        Err(error) => return Outcome { verdict: Verdict::Error, detail: error },
    }
    match reviewed(repo, &checkout, &name, &lock, env, print) {
        Ok(outcome) if outcome.verdict == Verdict::Started => outcome,
        Ok(outcome) => {
            let _ = fs::remove_file(&lock);
            outcome
        }
        Err(detail) => {
            let _ = fs::remove_file(&lock);
            Outcome { verdict: Verdict::Error, detail }
        }
    }
}

fn reviewed(repo: &str, checkout: &Path, name: &str, lock: &Path, env: &TickEnv, print: &mut dyn FnMut(String)) -> Result<Outcome, String> {
    let checkout_arg = checkout.display().to_string();
    let fetched = run(&words(&["git", "-C", &checkout_arg, "fetch", "-q", "origin"]), checkout);
    if fetched.code != 0 {
        return Err(format!("fetch origin failed: {}", fetched.stderr.trim()));
    }
    let code_health = |verb: &str| {
        let mut argv = env.code_health.clone();
        argv.extend(words(&[verb, "--repo", &checkout_arg, "--rev", "origin/main"]));
        if verb == "signal" {
            argv.push("--record".to_owned());
        }
        run(&argv, checkout)
    };
    let signal = code_health("signal");
    if signal.code == 1 {
        return Ok(Outcome { verdict: Verdict::Quiet, detail: "quiet".to_owned() });
    }
    let reasons: Vec<&str> = signal.stdout.lines().filter(|line| line.starts_with("FIRE")).collect();
    if signal.code != 0 || reasons.is_empty() {
        return Err(format!("signal exit {}: {}", signal.code, signal.stderr.trim()));
    }
    let report = code_health("report");
    if report.code != 0 {
        return Err(format!("report exit {}: {}", report.code, report.stderr.trim()));
    }
    let state = env.state_root.join(repo);
    let report_path = state.join("review-report.txt");
    fs::write(&report_path, &report.stdout).map_err(|error| format!("{}: {error}", report_path.display()))?;
    fs::create_dir_all(&env.handoffs).map_err(|error| format!("{}: {error}", env.handoffs.display()))?;
    let brief = env.handoffs.join(format!("{name}-brief.md"));
    let date = iso(env.now)[..10].to_owned();
    let values = [
        ("repo", repo.to_owned()),
        ("checkout", checkout_arg.clone()),
        ("lock", lock.display().to_string()),
        ("report", report_path.display().to_string()),
        ("reasons", reasons.iter().map(|line| format!("- {line}")).collect::<Vec<_>>().join("\n")),
        ("list", env.handoffs.join(format!("{name}-list.md")).display().to_string()),
        ("date", date),
    ];
    fs::write(&brief, fill(&BRIEF, &values)).map_err(|error| format!("{}: {error}", brief.display()))?;
    let prompt = format!("Review the code-health report for {repo} and land the bounded maintenance changes it identifies.");
    let brief_arg = brief.display().to_string();
    let command = words(&[&env.north, "fleet", "launch", "--provider", "claude", "--name", name, "--system-prompt", &brief_arg, "--cwd", &checkout_arg, &prompt]);
    let started = if env.dry_run {
        print(format!("dry-run: {}", command.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ")));
        Ran { code: 0, stdout: String::new(), stderr: String::new() }
    } else {
        run(&command, checkout)
    };
    if started.code != 0 && format!("{}{}", started.stderr, started.stdout).contains("launch from another host") {
        let last = started.stderr.trim().lines().last().unwrap_or_default().to_owned();
        return Ok(Outcome { verdict: Verdict::Deferred, detail: format!("host saturated, the next tick retries: {last}") });
    }
    if started.code != 0 {
        let said = if started.stderr.is_empty() { &started.stdout } else { &started.stderr };
        return Err(format!("fleet launch exit {}: {}", started.code, said.trim()));
    }
    let dry = if env.dry_run { "dry-run " } else { "" };
    Ok(Outcome { verdict: Verdict::Started, detail: format!("{dry}{name} ({})", reasons.join("; ")) })
}

fn env_or(name: &str, fallback: impl FnOnce() -> String) -> String {
    std::env::var(name).ok().filter(|value| !value.is_empty()).unwrap_or_else(fallback)
}

fn default_env(dry_run: bool) -> TickEnv {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let under_home = |rel: &str| home.join(rel).display().to_string();
    let north = || std::env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string());
    TickEnv {
        code_root: env_or("CODE_HEALTH_CODE_ROOT", || under_home("code")).into(),
        state_root: env_or("CODE_HEALTH_STATE", || under_home(".local/state/code-health")).into(),
        handoffs: env_or("AGENTS_HANDOFFS", || under_home(".local/state/agents/handoffs")).into(),
        code_health: env_or("CODE_HEALTH_BIN", || "north code-health".to_owned()).split(' ').map(str::to_owned).collect(),
        north: env_or("CODE_HEALTH_NORTH_BIN", north),
        dry_run,
        now: OffsetDateTime::now_utc(),
    }
}

const USAGE: &str = "usage: north code-health tick [--dry-run] [--repos=a,b]";

pub(super) fn run_tick(arguments: &[String]) -> u8 {
    let mut dry_run = false;
    let mut repos: Vec<String> = ENROLLED.iter().map(|&repo| repo.to_owned()).collect();
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                println!("For each enrolled repo ({}): take the review lock, run `code-health signal`, and on FIRE", ENROLLED.join(", "));
                println!("save the report, fill harness/code-health/review-brief.md and launch one fleet session.");
                println!("--dry-run prints the launch command instead of running it.");
                return 0;
            }
            "--dry-run" => dry_run = true,
            other => match other.strip_prefix("--repos=") {
                Some(list) => repos = list.split(',').filter(|repo| !repo.is_empty()).map(str::to_owned).collect(),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
        }
    }
    let env = default_env(dry_run);
    let mut failed = false;
    let mut print = |line: String| println!("{line}");
    for repo in &repos {
        let outcome = tick_repo(repo, &env, &mut print);
        failed |= outcome.verdict == Verdict::Error;
        println!("{repo} {}: {}", outcome.verdict.word(), outcome.detail);
    }
    u8::from(failed)
}
