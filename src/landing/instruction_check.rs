use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::platform::git;

const LIMIT: usize = 100;
const CATALOG: &str = "dotfiles/agents/catalog-config.json";
const USAGE: &str = "usage: agent-instruction-check [-h] [--repo DIR] [--ref REF]

Refuse repository AGENTS.md over 100 lines.

options:
  --repo DIR  check a repository's published Git tree (default: current directory)
  --ref REF   Git snapshot to check (default: HEAD)

Move detail to CLI --help, help TOPIC, or indexed docs; keep AGENTS.md as a router.";

struct Options {
    repo: String,
    reference: String,
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options { repo: ".".into(), reference: "HEAD".into() };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let slot = match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--repo" => &mut options.repo,
            "--ref" => &mut options.reference,
            other => return Err(format!("unrecognized arguments: {other}")),
        };
        *slot = rest.next().ok_or_else(|| format!("argument {flag}: expected one argument"))?.clone();
    }
    Ok(Some(options))
}

fn git_output(repo: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    git::stdout(Path::new(repo), args, |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn baseline_paths(catalog: &str) -> Result<BTreeSet<String>, String> {
    let catalog: Value = serde_json::from_str(catalog).map_err(|error| error.to_string())?;
    let mut paths = BTreeSet::new();
    for entry in catalog.get("baselines").and_then(Value::as_array).into_iter().flatten() {
        let owner = entry.get("owner").ok_or("missing owner")?;
        if owner.get("repo").and_then(Value::as_str).ok_or("missing owner.repo")? == "nixos-config" {
            paths.insert(owner.get("path").and_then(Value::as_str).ok_or("missing owner.path")?.to_owned());
        }
    }
    Ok(paths)
}

fn check(repo: &str, reference: &str) -> Result<(usize, Vec<String>), String> {
    let snapshot = text(git_output(repo, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")])?)?.trim().to_owned();
    let listing = git_output(repo, &["ls-tree", "-r", "--name-only", "-z", &snapshot])?;
    let paths: BTreeSet<String> = listing.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| text(path.to_vec())).collect::<Result<_, _>>()?;
    let baselines = if paths.contains(CATALOG) {
        baseline_paths(&text(git_output(repo, &["show", &format!("{snapshot}:{CATALOG}")])?)?)?
    } else {
        BTreeSet::new()
    };
    let (mut checked, mut failures) = (0, Vec::new());
    for path in paths.iter().filter(|path| Path::new(path).file_name().is_some_and(|name| name == "AGENTS.md") && !baselines.contains(*path)) {
        checked += 1;
        let lines = text(git_output(repo, &["show", &format!("{snapshot}:{path}")])?)?.lines().count();
        if lines > LIMIT {
            failures.push(format!("{path}@{snapshot}: {lines} lines, limit {LIMIT}; move detail to CLI --help, help TOPIC, or indexed docs and keep AGENTS.md as a router"));
        }
    }
    Ok((checked, failures))
}

pub(crate) fn run(args: &[String]) -> u8 {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprintln!("{}\nagent-instruction-check: error: {message}", USAGE.lines().next().unwrap_or_default());
            return 2;
        }
    };
    match check(&options.repo, &options.reference) {
        Err(error) => {
            eprintln!("agent-instruction-check: cannot check instructions: {error}");
            2
        }
        Ok((_, failures)) if !failures.is_empty() => {
            for failure in failures {
                eprintln!("agent-instruction-check: {failure}");
            }
            1
        }
        Ok((checked, _)) => {
            println!("agent-instruction-check: {checked} instruction files within limits");
            0
        }
    }
}
