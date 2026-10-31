use crate::autoland_status::{deletion_hold, git_output, landed_commit, read_gh, status};
use crate::foundation::clock::Timestamp;
use std::process::{Command, Stdio};

mod scratch;

pub use scratch::{ScratchPush, Witness};

fn lane_land_branch() -> Option<String> {
    let lane = git_output(&["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let sha = git_output(&["rev-parse", "--verify", "HEAD^{commit}"])?;
    let lane: String = lane
        .trim()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"._-".contains(&byte) {
                char::from(byte)
            } else {
                '-'
            }
        })
        .collect();
    Some(format!("claude/land-{lane}-{}", sha.trim().get(..12)?))
}

// An amended commit keeps its author and author date, so a reworded re-landing still matches.
fn authorship(line: &str) -> (&str, &str) {
    let mut fields = line.splitn(3, ' ');
    let stamp = fields.next().unwrap_or_default();
    let email = fields.next().unwrap_or_default();
    let author = line.get(..stamp.len() + 1 + email.len()).unwrap_or(line);
    (author, fields.next().unwrap_or_default())
}

fn superseded(tip: &str, destination: &str) -> Option<bool> {
    let lane = git_output(&[
        "rev-list",
        "--no-merges",
        "--format=%H %at %ae %s",
        "--no-commit-header",
        tip,
        &format!("^{destination}"),
    ])?;
    let base = git_output(&["merge-base", tip, destination])?;
    let landed = git_output(&[
        "log",
        "--no-merges",
        "--format=%at %ae %s",
        &format!("{}..{destination}", base.trim()),
    ])?;
    Some(lane.lines().all(|line| {
        let (sha, rest) = line.split_once(' ').unwrap_or((line, ""));
        let (author, subject) = authorship(rest);
        landed.lines().any(|other| {
            let (landed_author, landed_subject) = authorship(other);
            landed_subject == subject || landed_author == author
        }) || landed_commit(sha, destination).is_some()
    }))
}

enum Abandon {
    LandBranch(Option<String>),
    Superseded(String),
    Scratch(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Rule {
    LandedOrHandedBack,
    Superseded,
    Scratch,
}

impl Abandon {
    fn parse(arguments: &[String]) -> Option<Self> {
        match arguments {
            [] => Some(Self::LandBranch(lane_land_branch())),
            [flag, branch] if flag == "--superseded" => Some(Self::Superseded(branch.clone())),
            [branch] if branch.starts_with("scratch/") => Some(Self::Scratch(branch.clone())),
            [branch] => Some(Self::LandBranch(Some(branch.clone()))),
            _ => None,
        }
    }

    fn branch(self) -> Option<(String, Rule)> {
        match self {
            Self::LandBranch(branch) => branch
                .filter(|branch| branch.starts_with("claude/land-"))
                .map(|branch| (branch, Rule::LandedOrHandedBack)),
            Self::Superseded(branch) => (branch.starts_with("claude/")
                || branch.starts_with("farm/"))
            .then_some((branch, Rule::Superseded)),
            Self::Scratch(branch) => Some((branch, Rule::Scratch)),
        }
    }
}

pub fn abandon(arguments: &[String], witness: &dyn Witness) -> u8 {
    let Some(request) = Abandon::parse(arguments) else {
        eprintln!("Usage: north land-abandon [BRANCH | scratch/NAME | --superseded BRANCH]");
        return 2;
    };
    let Some((branch, rule)) = request.branch() else {
        eprintln!(
            "safe-push: abandon refuses anything but a claude/land-* or scratch/* branch (claude/* or farm/* with --superseded); NOT deleted."
        );
        return 1;
    };
    let head = git_output(&["ls-remote", "--symref", "origin", "HEAD"]).unwrap_or_default();
    let destination = head.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")?
            .strip_suffix("\tHEAD")
            .map(str::to_owned)
    });
    let tip = git_output(&["ls-remote", "origin", &format!("refs/heads/{branch}")])
        .and_then(|line| line.split_whitespace().next().map(str::to_owned));
    let (Some(destination), Some(tip)) = (destination, tip) else {
        eprintln!("safe-push: origin has no {branch} or no default branch; NOT deleted.");
        return 1;
    };
    if let Some(hold) = deletion_hold(&branch, &tip, Timestamp::now().unix_millis(), read_gh) {
        eprintln!("safe-push: {hold}; NOT deleted.");
        return 1;
    }
    let verdict = match verdict(rule, &branch, &tip, &destination, witness) {
        Ok(verdict) => verdict,
        Err(refusal) => {
            eprintln!("safe-push: {branch}{refusal}; NOT deleted.");
            return 1;
        }
    };
    eprintln!("safe-push: abandoning {branch}: {verdict}");
    if !delete_remote_branch(&branch, &tip) {
        eprintln!("safe-push: could not delete {branch} from origin.");
        return 1;
    }
    println!("safe-push: deleted {branch} from origin");
    0
}

fn verdict(rule: Rule, branch: &str, tip: &str, destination: &str, witness: &dyn Witness) -> Result<String, String> {
    let tracking = format!("refs/remotes/origin/{destination}");
    let fetched = git_output(&[
        "fetch",
        "-q",
        "--no-tags",
        "origin",
        &format!("+refs/heads/{destination}:{tracking}"),
        &format!("refs/heads/{branch}"),
    ]);
    if rule == Rule::Scratch {
        return fetched
            .ok_or_else(|| "could not fetch its tip".to_owned())
            .and_then(|_| scratch::verdict(&scratch::observe(branch, tip, witness)))
            .map_err(|refusal| format!(": {refusal}"));
    }
    if fetched.is_some() && superseded(tip, &tracking) == Some(true) {
        return Ok(format!("every lane commit is on origin/{destination} by subject or patch"));
    }
    if rule == Rule::LandedOrHandedBack {
        let state = status(tip, branch, read_gh);
        if matches!(state.split('\t').next(), Some("error" | "failure")) {
            return Ok(format!("Autoland handed it back: {}", state.replace('\t', " ")));
        }
        return Err(format!(" is not superseded on origin/{destination} and not handed back by Autoland"));
    }
    Err(format!(" is not superseded on origin/{destination}"))
}

fn delete_remote_branch(branch: &str, tip: &str) -> bool {
    Command::new("git")
        .args([
            "push",
            "-q",
            &format!("--force-with-lease=refs/heads/{branch}:{tip}"),
            "origin",
            &format!(":refs/heads/{branch}"),
        ])
        .env("SAFE_PUSH_ACTIVE", "1")
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
