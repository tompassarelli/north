mod consumed;
mod unconsumed;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::repo::{git_checked, git_ok, git_out};

const USAGE: &str = "Usage: pin-retire [--dry-run] --consumer-main PATH [--consumer-main PATH...] -- PIN_PATH
       pin-retire [--dry-run] --unconsumed -- PIN_PATH

Retire one clean detached pin after read-only checks prove every repository
consumer named by PIN_PATH.pin has moved. The sidecar must contain one exact
`consumer-main: ~/code/.../main` record per repository consumer, and the
--consumer-main arguments must match that set. PIN_PATH must be
~/code/.../pins/<40-hex-object-id>.

  --consumer-main    name every consumer repository's clean, published main
  --unconsumed       retire a pin with no .pin sidecar after proving its HEAD
                     matches its name (or a valid pin holds that HEAD), the
                     commit is reachable from origin/main, refs/archive/* or a
                     local branch, no ~/code main or lane references it, and
                     its tree is clean; prints the proof
  --dry-run          validate and print the retirement without changing files";

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ProofError {
    DuplicateArguments,
    DuplicateSidecar,
    Mismatch,
}

pub(super) fn consumer_proof(supplied: &[String], declared: &[String]) -> Result<(), ProofError> {
    let supplied_set = supplied.iter().collect::<BTreeSet<_>>();
    let declared_set = declared.iter().collect::<BTreeSet<_>>();
    if supplied_set.len() != supplied.len() {
        Err(ProofError::DuplicateArguments)
    } else if declared_set.len() != declared.len() {
        Err(ProofError::DuplicateSidecar)
    } else if supplied_set != declared_set {
        Err(ProofError::Mismatch)
    } else {
        Ok(())
    }
}

struct Request {
    dry_run: bool,
    unconsumed: bool,
    consumers: Vec<String>,
    pin: String,
}

enum Parsed {
    Help,
    Request(Request),
}

fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut request = Request { dry_run: false, unconsumed: false, consumers: Vec::new(), pin: String::new() };
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            "--consumer-main" => request.consumers.push(remaining.next().ok_or("--consumer-main needs a path")?.clone()),
            "--dry-run" => request.dry_run = true,
            "--unconsumed" => request.unconsumed = true,
            "--" => {
                let rest = remaining.by_ref().collect::<Vec<_>>();
                let [pin] = rest.as_slice() else {
                    return Err("exactly one pin path must follow --".into());
                };
                request.pin = (*pin).clone();
                break;
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            flag if flag.starts_with('-') => return Err(format!("unknown argument '{flag}'")),
            path if request.pin.is_empty() => request.pin = path.to_string(),
            _ => return Err("exactly one pin path is required".into()),
        }
    }
    if request.pin.is_empty() {
        return Err("a pin path is required".into());
    }
    if request.unconsumed && !request.consumers.is_empty() {
        return Err("--unconsumed and --consumer-main are exclusive".into());
    }
    if !request.unconsumed && request.consumers.is_empty() {
        return Err("name every manifest consumer with --consumer-main".into());
    }
    Ok(Parsed::Request(request))
}

pub fn run(arguments: &[String]) -> u8 {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    match execute(&home, arguments) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(message) => {
            eprintln!("pin-retire: {message}");
            1
        }
    }
}

pub(super) fn execute(home: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let request = match parse(arguments)? {
        Parsed::Help => return Ok(vec![USAGE.to_string()]),
        Parsed::Request(request) => request,
    };
    let code_root = home.join("code").canonicalize().map_err(|_| format!("{} is unavailable", home.join("code").display()))?;
    let pin = expand(home, &request.pin).canonicalize().map_err(|_| format!("pin does not exist: {}", request.pin))?;
    if !pin.starts_with(&code_root) {
        return Err(format!("pin is outside {}: {}", code_root.display(), pin.display()));
    }
    let pins_dir = pin.parent().filter(|dir| dir.file_name().is_some_and(|name| name == "pins"));
    let pins_dir = pins_dir.ok_or_else(|| format!("path is not directly under a pins/ slot: {}", pin.display()))?;
    let container = pins_dir.parent().unwrap_or(pins_dir);
    let target = Target {
        main: container.join("main"),
        container: container.to_path_buf(),
        pins_dir: pins_dir.to_path_buf(),
        leaf: pin.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        pin,
        code_root,
        dry_run: request.dry_run,
    };
    if request.unconsumed {
        target.retire_unconsumed()
    } else {
        target.retire_consumed(home, &request.consumers)
    }
}

fn expand(home: &Path, raw: &str) -> PathBuf {
    raw.strip_prefix("~/").map_or_else(|| PathBuf::from(raw), |rest| home.join(rest))
}

#[derive(Clone)]
struct Target {
    pin: PathBuf,
    pins_dir: PathBuf,
    container: PathBuf,
    main: PathBuf,
    leaf: String,
    code_root: PathBuf,
    dry_run: bool,
}

struct Consumer {
    main: PathBuf,
    head: String,
}

impl Target {
    fn registered_head(&self) -> Option<String> {
        let listing = git_out(&self.main, &["worktree", "list", "--porcelain"])?;
        listing.split("\n\n").find_map(|block| {
            let lines = block.lines().collect::<Vec<_>>();
            let path = lines.iter().find_map(|line| line.strip_prefix("worktree "))?;
            let head = lines.iter().find_map(|line| line.strip_prefix("HEAD "))?;
            (Path::new(path) == self.pin && lines.contains(&"detached")).then(|| head.to_string())
        })
    }

    fn registered(&self) -> bool {
        git_out(&self.main, &["worktree", "list", "--porcelain"])
            .is_some_and(|listing| listing.lines().any(|line| line.strip_prefix("worktree ").is_some_and(|path| Path::new(path) == self.pin)))
    }

    fn require_main(&self) -> Result<(), String> {
        if self.main.join(".git").exists() { Ok(()) } else { Err(format!("container has no main checkout: {}", self.container.display())) }
    }

    fn dirty(&self) -> bool {
        git_out(&self.pin, &["status", "--porcelain", "--untracked-files=all"]).is_none_or(|status| !status.is_empty())
    }

    fn pin_head(&self) -> Option<String> {
        git_out(&self.pin, &["rev-parse", "--verify", "HEAD"])
    }

    fn attached(&self) -> bool {
        git_ok(&self.pin, &["symbolic-ref", "--quiet", "HEAD"])
    }

    fn remove(&self) -> Result<(), String> {
        git_checked(&self.main, &["worktree", "remove", &self.pin.to_string_lossy()])?;
        if self.pin.symlink_metadata().is_ok() {
            return Err(format!("worktree removal left the pin path present: {}", self.pin.display()));
        }
        Ok(())
    }

    fn sidecar(&self) -> PathBuf {
        self.pins_dir.join(format!("{}.pin", self.leaf))
    }
}
