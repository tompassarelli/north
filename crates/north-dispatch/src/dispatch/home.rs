//! The dispatch host, South's `[placement] dispatch_host`: the one host whose dispatch records (launches, host needs,
//! launch holds, wait stops) a plan reads, so a dry-run elsewhere runs there instead of planning from stale copies.
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

/// Runs `north fleet dispatch --dry-run` on the dispatch host with this terminal, returning its exit code.
pub(super) fn dry_run_there(here: &str, host: &str, ssh: &str) -> u8 {
    println!("north fleet dispatch: {here} is not the dispatch host, so this dry-run runs on {host}, whose dispatch records the plan reads");
    let status = std::process::Command::new("ssh").args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "north", "fleet", "dispatch", "--dry-run"]).status();
    match status {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(2),
        Err(error) => {
            eprintln!("north fleet dispatch: ssh {ssh}: {error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::elsewhere;

    #[test]
    fn a_dry_run_on_the_laptop_is_sent_to_the_dispatch_host_and_one_there_runs_in_place() {
        let cfg: toml_edit::DocumentMut = "[placement]\ndispatch_host = \"nexus\"\n[hosts.nexus]\n[hosts.gex45]\nssh = \"root@gex45\"\n[hosts.whiterabbit]\n".parse().unwrap();
        assert_eq!(elsewhere(&cfg, "whiterabbit"), Some(("nexus".to_owned(), "nexus".to_owned())));
        assert_eq!(elsewhere(&cfg, "nexus"), None);
    }
}
