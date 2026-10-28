use crate::autoland_status::{git_output, landed_commit, read_gh, status};
use std::process::{Command, Stdio};

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

fn superseded(tip: &str, destination: &str) -> Option<bool> {
    let lane = git_output(&[
        "rev-list",
        "--no-merges",
        "--format=%H %s",
        "--no-commit-header",
        tip,
        &format!("^{destination}"),
    ])?;
    let base = git_output(&["merge-base", tip, destination])?;
    let landed = git_output(&[
        "log",
        "--no-merges",
        "--format=%s",
        &format!("{}..{destination}", base.trim()),
    ])?;
    Some(lane.lines().all(|line| {
        let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
        landed.lines().any(|other| other == subject) || landed_commit(sha, destination).is_some()
    }))
}

enum Abandon {
    LandBranch(Option<String>),
    Superseded(String),
}

impl Abandon {
    fn parse(arguments: &[String]) -> Option<Self> {
        match arguments {
            [] => Some(Self::LandBranch(lane_land_branch())),
            [flag, branch] if flag == "--superseded" => Some(Self::Superseded(branch.clone())),
            [branch] => Some(Self::LandBranch(Some(branch.clone()))),
            _ => None,
        }
    }

    fn branch(self) -> Option<(String, bool)> {
        match self {
            Self::LandBranch(branch) => branch
                .filter(|branch| branch.starts_with("claude/land-"))
                .map(|branch| (branch, false)),
            Self::Superseded(branch) => (branch.starts_with("claude/")
                || branch.starts_with("farm/"))
            .then_some((branch, true)),
        }
    }
}

pub fn abandon(arguments: &[String]) -> u8 {
    let Some(request) = Abandon::parse(arguments) else {
        eprintln!("Usage: north land-abandon [BRANCH | --superseded BRANCH]");
        return 2;
    };
    let Some((branch, only_superseded)) = request.branch() else {
        eprintln!(
            "safe-push: abandon refuses anything but a claude/land-* branch (claude/* or farm/* with --superseded); NOT deleted."
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
    let tracking = format!("refs/remotes/origin/{destination}");
    let fetched = git_output(&[
        "fetch",
        "-q",
        "--no-tags",
        "origin",
        &format!("+refs/heads/{destination}:{tracking}"),
        &format!("refs/heads/{branch}"),
    ]);
    let state = status(&tip, &branch, read_gh);
    let verdict = if fetched.is_some() && superseded(&tip, &tracking) == Some(true) {
        format!("every lane commit is on origin/{destination} by subject or patch")
    } else if !only_superseded && matches!(state.split('\t').next(), Some("error" | "failure")) {
        format!("Autoland handed it back: {}", state.replace('\t', " "))
    } else {
        eprintln!(
            "safe-push: {branch} is not superseded on origin/{destination}{}; NOT deleted.",
            if only_superseded {
                ""
            } else {
                " and not handed back by Autoland"
            }
        );
        return 1;
    };
    eprintln!("safe-push: abandoning {branch}: {verdict}");
    if !delete_remote_branch(&branch, &tip) {
        eprintln!("safe-push: could not delete {branch} from origin.");
        return 1;
    }
    println!("safe-push: deleted {branch} from origin");
    0
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
