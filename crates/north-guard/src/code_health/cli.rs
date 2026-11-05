use super::check::check;
use super::config::at_rev;
use super::functions::ast_grep_available;
use super::gate::{Violation, gate};
use super::report::{build, format, trend_row};
use super::signal::{review_done, signal};
use super::snapshot::{Snapshot, git};
use super::state::State;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) const USAGE: &str = "usage: north code-health <command> [--repo DIR] [--config FILE] [--rev REV] [--json]
  report [--save]          full metrics for REV's tree (default HEAD); --save writes the baseline and appends the trend
  gate BASE HEAD           new structural debt between two commits (exit 0 pass, 1 debt, 2 error)
  check FILE               edit-time check of one worktree file against HEAD (prints nothing when clean)
  signal                   FIRE <reason> lines for REV (exit 0) or quiet (exit 1) for the recurring review
  review-done [--sha SHA]  record the review marker
  imports                  resolve imports of files given as JSON on stdin
  habits [--repo DIR]      the Rust habit checks
  tick [--dry-run]         the hourly review tick
  crate-lines [BASE HEAD]  workspace crate line caps (the default with no command)
  layers BASE HEAD         module and crate DAG breaches between two commits
  build-time ...           see north code-health build-time --help";

#[derive(Default)]
struct Arguments {
    command: Option<String>,
    positional: Vec<String>,
    repo: Option<PathBuf>,
    config: Option<PathBuf>,
    json: bool,
    save: bool,
    sha: Option<String>,
    rev: Option<String>,
}

fn parse(arguments: &[String], cwd: &Path) -> Result<Arguments, String> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = || rest.next().cloned().ok_or_else(|| format!("{argument} needs a value"));
        match argument.as_str() {
            "--repo" => parsed.repo = Some(cwd.join(value()?)),
            "--config" => parsed.config = Some(cwd.join(value()?)),
            "--sha" => parsed.sha = Some(value()?),
            "--rev" => parsed.rev = Some(value()?),
            "--json" => parsed.json = true,
            "--save" => parsed.save = true,
            "-h" | "--help" => parsed.command = Some("help".to_owned()),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ if parsed.command.is_none() => parsed.command = Some(argument.clone()),
            _ => parsed.positional.push(argument.clone()),
        }
    }
    Ok(parsed)
}

pub(super) struct Io<'a> {
    pub(super) out: &'a mut dyn Write,
    pub(super) err: &'a mut dyn Write,
    pub(super) cwd: PathBuf,
    pub(super) state: State,
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    let cwd = std::env::current_dir().unwrap_or_default();
    run_with(arguments, &mut Io { out: &mut out, err: &mut err, cwd, state: State::from_env() })
}

pub(super) fn run_with(arguments: &[String], io: &mut Io) -> u8 {
    match dispatch(arguments, io) {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(io.err, "north code-health: {message}");
            2
        }
    }
}

fn violations(io: &mut Io, found: &[Violation], json: bool) -> Result<(), String> {
    let text = if json { serde_json::to_string(found).map_err(|error| error.to_string())? + "\n" } else { found.iter().map(|v| v.shown() + "\n").collect() };
    io.out.write_all(text.as_bytes()).map_err(|error| error.to_string())
}

fn print(io: &mut Io, text: &str) -> Result<(), String> {
    writeln!(io.out, "{text}").map_err(|error| error.to_string())
}

fn dispatch(arguments: &[String], io: &mut Io) -> Result<u8, String> {
    let a = parse(arguments, &io.cwd)?;
    let Some(command) = a.command.as_deref().filter(|command| *command != "help") else {
        print(io, USAGE)?;
        return Ok(if a.command.is_some() { 0 } else { 2 });
    };
    let repo = PathBuf::from(git(a.repo.as_deref().unwrap_or(&io.cwd), &["rev-parse", "--show-toplevel"])?.trim());
    let rev = a.rev.as_deref().unwrap_or("HEAD");
    let config = a.config.as_deref();
    match command {
        "report" => {
            if !ast_grep_available() {
                return Err("ast-grep is not on PATH".to_owned());
            }
            let report = build(&repo, rev, &at_rev(&Snapshot::new(&repo, rev)?, config)?)?;
            if a.save {
                io.state.save(&repo, &report, &trend_row(&report))?;
            }
            let text = if a.json { serde_json::to_string(&report).map_err(|error| error.to_string())? } else { format(&report, &io.state) };
            print(io, &text)?;
            Ok(0)
        }
        "gate" => {
            let [base, head] = a.positional.as_slice() else { return Err("gate needs BASE HEAD".to_owned()) };
            if !ast_grep_available() {
                let _ = writeln!(io.err, "code-health: ast-grep is not on PATH; gate skipped");
                return Ok(0);
            }
            let result = gate(&io.state, &repo, base, head, config)?;
            if let Some(warning) = &result.warning {
                let _ = writeln!(io.err, "{warning}");
            }
            violations(io, &result.violations, a.json)?;
            Ok(u8::from(!result.violations.is_empty()))
        }
        "check" => {
            let [file] = a.positional.as_slice() else { return Err("check needs FILE".to_owned()) };
            if !ast_grep_available() {
                return Ok(0);
            }
            let found = check(&repo, &io.cwd.join(file), config)?;
            if !(a.json && found.is_empty()) {
                violations(io, &found, a.json)?;
            }
            Ok(u8::from(!found.is_empty()))
        }
        "signal" => {
            let reasons = signal(&io.state, &repo, &at_rev(&Snapshot::new(&repo, rev)?, config)?, north_base::foundation::clock::Timestamp::now(), rev)?;
            let text = if a.json {
                serde_json::json!({ "fire": !reasons.is_empty(), "reasons": reasons }).to_string()
            } else if reasons.is_empty() {
                "quiet".to_owned()
            } else {
                reasons.iter().map(|reason| format!("FIRE {reason}")).collect::<Vec<_>>().join("\n")
            };
            print(io, &text)?;
            Ok(u8::from(reasons.is_empty()))
        }
        "review-done" => {
            let sha = a.sha.as_deref().unwrap_or(rev);
            let explicit = match config {
                Some(path) => Some(at_rev(&Snapshot::new(&repo, sha)?, Some(path))?),
                None => None,
            };
            review_done(&io.state, &repo, sha, explicit.as_ref())?;
            Ok(0)
        }
        other => Err(format!("unknown command {other}\n{USAGE}")),
    }
}
