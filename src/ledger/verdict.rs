use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::Books;
use crate::foundation::clock::Timestamp;
use crate::platform::git;

use super::{CheckRun, Ledger};

const MAIN: &str = "origin/main";

pub fn landed(ledger: &Ledger, repo: &Path, task_ref: &str, revision: &str) -> NorthResult<String> {
    git::stdout(repo, &["fetch", "-q", "origin", "main"], |output| refusal("git fetch origin main failed", output))
        .map_err(NorthError::Refused)?;
    let commit = format!("{revision}^{{commit}}");
    let resolved = git::stdout(repo, &["rev-parse", "--verify", "--end-of-options", &commit], |output| {
        refusal(&format!("{revision} is not a commit here"), output)
    })
    .map_err(NorthError::Refused)?;
    let sha = String::from_utf8_lossy(&resolved).trim().to_owned();
    let ancestry = git::run(repo, &["merge-base", "--is-ancestor", &sha, MAIN])?;
    match ancestry.status.code() {
        Some(0) => {}
        Some(1) => return Err(NorthError::Refused(format!("{sha} is not on freshly fetched {MAIN}; nothing recorded"))),
        _ => return Err(NorthError::Refused(refusal("git merge-base failed", &ancestry))),
    }
    ledger.record_landing(task_ref, &sha, u64::try_from(Timestamp::now().unix_millis()).unwrap_or(0), None, None)?;
    Ok(sha)
}

pub fn check(ledger: &Ledger, task_ref: &str, argv: &[String]) -> NorthResult<i32> {
    let (program, arguments) = argv.split_first().ok_or_else(|| NorthError::Usage("task check needs a command after --".into()))?;
    let cwd = std::env::current_dir()?;
    let head = git::stdout(&cwd, &["rev-parse", "HEAD"], |_| String::new()).ok();
    let head = head.map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned());
    let exit = match Command::new(program).args(arguments).status() {
        Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(error) => {
            eprintln!("north task check: {program}: {error}");
            127
        }
    };
    ledger.record_check(&CheckRun {
        task_ref,
        command: &argv.join(" "),
        cwd: &cwd,
        head_sha: head.as_deref(),
        exit,
        ran_ms: Timestamp::now().unix_millis(),
    })?;
    Ok(exit)
}

fn refusal(reason: &str, output: &std::process::Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr);
    format!("{reason}; nothing recorded: {}", detail.trim())
}
