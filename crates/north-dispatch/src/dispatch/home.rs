//! The dispatch host, South's `[placement] dispatch_host`: the one host whose dispatch records (launches, host needs,
//! launch holds, wait stops) a plan reads, so a run elsewhere, dry or real, runs there instead of planning from stale copies.
use crate::pacing::placement::gate;
use toml_edit::{DocumentMut, Item};

/// The dispatch host and the ssh destination that reaches it, when it is not `here`.
pub(super) fn elsewhere(cfg: &DocumentMut, here: &str) -> Option<(String, String)> {
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
pub(super) fn run_there(here: &str, host: &str, ssh: &str, dry_run: bool) -> u8 {
    let kind = if dry_run { "dry-run" } else { "run" };
    println!("north fleet dispatch: {here} is not the dispatch host, so this {kind} runs on {host}, whose dispatch records the plan reads");
    match std::process::Command::new("ssh").args(remote_words(ssh, dry_run)).status() {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(2),
        Err(error) => {
            eprintln!("north fleet dispatch: ssh {ssh}: {error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{elsewhere, remote_words};
    use crate::dispatch::freeze::frozen;
    use crate::dispatch::issues::IssueRef;
    use crate::dispatch::Outcome;
    use crate::dispatch::tier::{Route, Tier};

    #[test]
    fn a_dry_run_on_the_laptop_is_sent_to_the_dispatch_host_and_one_there_runs_in_place() {
        let cfg: toml_edit::DocumentMut = "[placement]\ndispatch_host = \"nexus\"\n[hosts.nexus]\n[hosts.gex45]\nssh = \"root@gex45\"\n[hosts.whiterabbit]\n".parse().unwrap();
        assert_eq!(elsewhere(&cfg, "whiterabbit"), Some(("nexus".to_owned(), "nexus".to_owned())));
        assert_eq!(elsewhere(&cfg, "nexus"), None);
    }

    #[test]
    fn a_run_launches_exactly_what_its_dry_run_plans_or_names_the_guard() {
        let real = remote_words("nexus", false);
        let dry = remote_words("nexus", true);
        assert_eq!(dry[..real.len()], real[..], "a real run elsewhere runs on the dispatch host the very command its dry run does");
        assert_eq!(dry[real.len()..], ["--dry-run"]);

        let item = IssueRef { repo: "north".into(), number: 1054 };
        let planned = crate::dispatch::plan::Launch {
            item: item.clone(),
            host: "gex45".into(),
            account: "claude:e".into(),
            brief: String::new(),
            route: Route { tier: Tier::Sonnet, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() },
            cloud: false,
            local: None,
        };
        let outcome = Outcome { launches: vec![planned], ready: vec![item], free: 16, ..Outcome::default() };
        let guard = "skip north#1054: dispatch was paused while this run was working: account b is the last headroom".to_owned();
        let page = frozen(&outcome, std::slice::from_ref(&guard), &[]).expect("the one planned launch was stopped, so the run is frozen");
        assert!(page.contains(&guard), "{page}");
        assert_eq!(frozen(&outcome, &[], &[]), None, "a planned launch nothing stopped is a launch");
    }
}
