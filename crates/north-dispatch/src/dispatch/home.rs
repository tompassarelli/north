//! The dispatch host, South's `[placement] dispatch_host`: the one host whose dispatch records (launches, host needs,
//! launch holds, wait stops) a plan reads, so a run elsewhere, dry or real, runs there instead of planning from stale copies.
use north_pacing::pacing::placement::gate;
use toml_edit::{DocumentMut, Item};

/// The dispatch host and the ssh destination that reaches it, when it is not `here`.
pub fn elsewhere(cfg: &DocumentMut, here: &str) -> Option<(String, String)> {
    let host = cfg.get("placement")?.get("dispatch_host").and_then(Item::as_str)?;
    (host != here).then(|| {
        let ssh = gate::machines(cfg).into_iter().find(|machine| machine.name == host).map_or_else(|| host.to_owned(), |machine| machine.ssh);
        (host.to_owned(), ssh)
    })
}

/// The ssh words that run on `ssh` the same dispatch run this host was asked for.
fn remote_words(ssh: &str, dry_run: bool) -> Vec<String> {
    let mut words: Vec<String> = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "north", "fleet", "dispatch"].map(str::to_owned).into();
    words.extend(dry_run.then(|| "--dry-run".to_owned()));
    words
}

/// Runs `north fleet dispatch` (with `--dry-run` when asked) on the dispatch host with this terminal, returning its exit code.
pub(super) fn run_there(here: &str, host: &str, ssh: &str, dry_run: bool, drafted: &[super::IssueRef]) -> u8 {
    let kind = if dry_run { "dry-run" } else { "run" };
    println!("north fleet dispatch: {here} is not the dispatch host, so this {kind} runs on {host}, whose dispatch records the plan reads");
    let drafts = (!drafted.is_empty()).then(|| std::iter::once("--draft".to_owned()).chain(drafted.iter().map(ToString::to_string))).into_iter().flatten();
    match std::process::Command::new("ssh").args(remote_words(ssh, dry_run).into_iter().chain(drafts)).status() {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(2),
        Err(error) => {
            eprintln!("north fleet dispatch: ssh {ssh}: {error}");
            2
        }
    }
}
