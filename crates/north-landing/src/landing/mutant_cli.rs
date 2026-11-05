use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::mutant::{Env, Lease, Stop, git_bytes, repo_name, valid_id};
use super::mutant_sweep::{self, Issues, MatrixArgs, RetireArgs, SweepArgs};

const USAGE: &str = "Usage: north mutant record <id> | north mutant verify <base> <head>
       north mutant matrix [--mutants N] [--seed TEXT] [--timeout SECONDS_PER_TREE]
       north mutant retire --repo NAME --evidence TSV [--matrix TSV]... [--contract TEST]... [--date YYYY-MM-DD] [--days N]
       north mutant sweep --repo DIR... [--host NAME] [--mutants N] [--timeout SECONDS_PER_TREE] [--days N] [--issue OWNER_REPO#N] [--remote-north COMMAND]
Record the scratch copy diff against HEAD; verify each Test-kind kill claim on demand; mutate product code
and print which tests kill which mutants; propose the tests with no recent catch, no unique kill and no
listed contract; run both on a build host for each repository and add the proposal as a box on an issue.";

pub(super) fn process_env() -> Env {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let base = |name: &str, fallback: &str| match std::env::var_os(name) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home.join(fallback),
    };
    Env { state_home: base("XDG_STATE_HOME", ".local/state"), cache_home: base("XDG_CACHE_HOME", ".cache"), lease: lease() }
}

fn lease() -> Lease {
    let cgroup = fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    if cgroup.split('/').any(|part| part.starts_with("agent-capacity-") && part.trim_end().ends_with(".scope")) {
        return Lease::Inherited;
    }
    Lease::Host(north_base::platform::north_binary::north_binary())
}

pub fn run(arguments: &[String], issues: &dyn Issues) -> u8 {
    let env = process_env();
    let outcome = std::env::current_dir().map_err(Stop::from).and_then(|cwd| match arguments {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        [command, id] if command == "record" => record(&cwd, id, &env),
        [command, base, head] if command == "verify" => super::mutant_verify::verify(&cwd, base, head, &env, &mut io::stdout()),
        [command, rest @ ..] if command == "matrix" => matrix_args(rest).and_then(|args| mutant_sweep::matrix(&cwd, &args, &env, &mut io::stdout(), &mut io::stderr())),
        [command, rest @ ..] if command == "retire" => retire_args(rest).and_then(|args| mutant_sweep::retire(&args)).map(|line| println!("{line}")),
        [command, rest @ ..] if command == "sweep" => sweep_args(rest).and_then(|args| mutant_sweep::sweep(&args, &env, issues, &mut io::stdout())),
        _ => Err(Stop::Usage(USAGE.to_owned())),
    });
    match outcome {
        Ok(()) => 0,
        Err(Stop::Usage(message)) => {
            eprintln!("{message}");
            2
        }
        Err(Stop::Fail(message)) => {
            eprintln!("{message}");
            1
        }
        Err(Stop::Refused) => 1,
    }
}

pub(super) fn record(cwd: &Path, id: &str, env: &Env) -> Result<(), Stop> {
    if !valid_id(id) {
        return Err(Stop::Usage("mutant: invalid id".to_owned()));
    }
    let repo = repo_name(cwd)?;
    let diff = git_bytes(cwd, None, &["diff", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD"])?;
    if diff.is_empty() {
        return Err(Stop::Fail(format!("mutant: {id} has an empty diff; NOT recorded.")));
    }
    let patch = env.patch(&repo, id);
    fs::create_dir_all(env.mutants(&repo))?;
    let partial = PathBuf::from(format!("{}.tmp.{}", patch.display(), std::process::id()));
    fs::write(&partial, &diff)?;
    fs::rename(&partial, &patch)?;
    println!("mutant: recorded {}", patch.display());
    Ok(())
}

fn pairs(rest: &[String]) -> Result<Vec<(&str, &str)>, Stop> {
    rest.chunks(2)
        .map(|pair| match pair {
            [flag, value] if flag.starts_with("--") => Ok((flag.as_str(), value.as_str())),
            _ => Err(Stop::Usage(USAGE.to_owned())),
        })
        .collect()
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, Stop> {
    value.parse().map_err(|_| Stop::Usage(format!("mutant: {flag} needs a number, not {value}")))
}

fn matrix_args(rest: &[String]) -> Result<MatrixArgs, Stop> {
    let mut args = MatrixArgs { mutants: 40, seed: mutant_sweep::today(), timeout_seconds: 3600 };
    for (flag, value) in pairs(rest)? {
        match flag {
            "--mutants" => args.mutants = number(flag, value)?,
            "--seed" => args.seed = value.to_owned(),
            "--timeout" => args.timeout_seconds = number(flag, value)?,
            _ => return Err(Stop::Usage(USAGE.to_owned())),
        }
    }
    Ok(args)
}

fn retire_args(rest: &[String]) -> Result<RetireArgs, Stop> {
    let mut args = RetireArgs { repo: String::new(), matrices: Vec::new(), evidence: PathBuf::new(), contracts: Vec::new(), date: mutant_sweep::today(), days: 90 };
    for (flag, value) in pairs(rest)? {
        match flag {
            "--repo" => args.repo = value.to_owned(),
            "--evidence" => args.evidence = PathBuf::from(value),
            "--matrix" => args.matrices.push(PathBuf::from(value)),
            "--contract" => args.contracts.push(value.to_owned()),
            "--date" => args.date = value.to_owned(),
            "--days" => args.days = number(flag, value)?,
            _ => return Err(Stop::Usage(USAGE.to_owned())),
        }
    }
    if args.repo.is_empty() || args.evidence.as_os_str().is_empty() {
        return Err(Stop::Usage(USAGE.to_owned()));
    }
    Ok(args)
}

fn sweep_args(rest: &[String]) -> Result<SweepArgs, Stop> {
    let mut args = SweepArgs { repos: Vec::new(), host: "gex45".to_owned(), mutants: 40, timeout_seconds: 3600, days: 90, issue: None, remote_north: "north".to_owned() };
    for (flag, value) in pairs(rest)? {
        match flag {
            "--repo" => args.repos.push(PathBuf::from(value)),
            "--host" => args.host = value.to_owned(),
            "--mutants" => args.mutants = number(flag, value)?,
            "--timeout" => args.timeout_seconds = number(flag, value)?,
            "--days" => args.days = number(flag, value)?,
            "--remote-north" => args.remote_north = value.to_owned(),
            "--issue" => {
                let (repo, issue) = value.split_once('#').ok_or_else(|| Stop::Usage(format!("mutant: --issue {value} is not REPO#N")))?;
                args.issue = Some((repo.to_owned(), number(flag, issue)?));
            }
            _ => return Err(Stop::Usage(USAGE.to_owned())),
        }
    }
    if args.repos.is_empty() {
        return Err(Stop::Usage(USAGE.to_owned()));
    }
    Ok(args)
}
