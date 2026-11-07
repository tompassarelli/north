//! One proxy at a time: while Tom works in his own Claude window, that window directs the fleet, and a fleet session in the proxy
//! role on another conversation keeps reading its reports but neither messages nor launches sessions until the window is idle. A window
//! still open on the conversation of a proxy a take-over archived directs nothing, so the proxy that took over is in command at once.
use std::time::{Duration, SystemTime};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Role;
use crate::fleet::registry::Fleet;

use super::lineage;
use super::window_move;

const TYPING: Duration = Duration::from_secs(30 * 60);

pub(super) struct Window {
    pub conversation: String,
    pub account: String,
    pub active: SystemTime,
    pub pid: u32,
}

pub(in crate::cli) fn admit(fleet: &Fleet, caller: Option<&str>, act: &str) -> NorthResult<()> {
    admit_beside(fleet, caller, act, window_move::tom_windows, &window_move::own_ancestors(), SystemTime::now())
}

fn admit_beside(fleet: &Fleet, caller: Option<&str>, act: &str, windows: impl FnOnce() -> Vec<Window>, ancestors: &[u32], now: SystemTime) -> NorthResult<()> {
    let Some(caller) = caller.filter(|caller| lineage::from_proxy(fleet, Some(caller))) else { return Ok(()) };
    let (mine, windows): (Vec<Window>, Vec<Window>) = windows().into_iter().partition(|window| ancestors.contains(&window.pid));
    if let Some(window) = mine.first() {
        fleet.follow_conversation(caller, &window.conversation)?;
    }
    let own = fleet.sessions()?.into_iter().find(|record| record.name == caller).and_then(|record| record.conversation);
    let archived: Vec<String> = fleet.past_sessions().into_iter().filter(|record| record.role() == Role::Proxy).filter_map(|record| record.conversation).collect();
    let directing = windows.into_iter().filter(|window| own.as_deref() != Some(window.conversation.as_str()) && !archived.contains(&window.conversation));
    let Some(window) = directing.max_by_key(|window| window.active) else { return Ok(()) };
    let idle = now.duration_since(window.active).unwrap_or_default();
    if idle >= TYPING {
        return Ok(());
    }
    Err(NorthError::Refused(format!(
        "{caller} may not {act}: Tom is working in his own Claude window (conversation {} on account {}, last written {}m ago), and one proxy directs the fleet at a time. Keep reading reports and put what you would send in your final message; you direct again once that window has been idle for {}m",
        window.conversation,
        window.account,
        idle.as_secs() / 60,
        TYPING.as_secs() / 60
    )))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::fleet::runtimes::Runtimes;

    fn session(home: &Path, name: &str, role: &str) {
        let directory = home.join(".local/state/north/fleet").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 0, "host_pid": null, "conversation": format!("{name}-conversation"), "host_dir": "", "parent": null, "role": role, "stopped": false});
        std::fs::write(directory.join("session.json"), record.to_string()).unwrap();
    }

    #[test]
    fn a_second_proxy_is_refused_while_tom_types_into_his_own_window_and_directs_again_once_it_is_idle() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(crate::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap();
        session(home.path(), "proxy", "proxy");
        session(home.path(), "lead-a", "lead");
        let now = SystemTime::now();
        let window = |conversation: &'static str, idle_minutes: u64| move || vec![Window { conversation: conversation.to_owned(), account: "c".into(), active: now - Duration::from_secs(idle_minutes * 60), pid: 4242 }];
        let refused = admit_beside(&fleet, Some("proxy"), "message worker-1", window("tom-window", 2), &[], now).unwrap_err().user_message();
        assert!(refused.contains("proxy may not message worker-1") && refused.contains("tom-window on account c"), "{refused}");
        assert!(admit_beside(&fleet, Some("proxy"), "message worker-1", window("tom-window", 31), &[], now).is_ok(), "an idle window hands direction back");
        assert!(admit_beside(&fleet, Some("proxy"), "message worker-1", window("proxy-conversation", 2), &[], now).is_ok(), "Tom's window resuming the proxy's own conversation is the same proxy");
        assert!(admit_beside(&fleet, Some("lead-a"), "message worker-1", window("tom-window", 2), &[], now).is_ok(), "a lead directs its own workers");
        assert!(admit_beside(&fleet, Some("proxy"), "message worker-1", Vec::new, &[], now).is_ok());

        session(home.path(), "proxy-before", "proxy");
        std::fs::create_dir_all(home.path().join(".local/state/north/fleet/.archive")).unwrap();
        std::fs::rename(home.path().join(".local/state/north/fleet/proxy-before"), home.path().join(".local/state/north/fleet/.archive/proxy-before-1")).unwrap();
        assert!(admit_beside(&fleet, Some("proxy"), "message worker-1", window("proxy-before-conversation", 2), &[], now).is_ok(), "the window left on a proxy the take-over archived directs nothing");
        let both = || window("proxy-before-conversation", 1)().into_iter().chain(window("tom-window", 2)()).collect();
        assert!(admit_beside(&fleet, Some("proxy"), "message worker-1", both, &[], now).is_err(), "a window on any other conversation still directs");

        session(home.path(), "proxy", "proxy");
        assert!(admit_beside(&fleet, Some("proxy"), "launch worker-1", window("1f79cfb7", 2), &[], now).is_err(), "a window outside the caller's process tree directs");
        assert!(admit_beside(&fleet, Some("proxy"), "launch worker-1", window("1f79cfb7", 2), &[9, 4242], now).is_ok(), "the proxy's own window on a new conversation is the proxy");
        assert_eq!(fleet.session("proxy").unwrap().conversation.as_deref(), Some("1f79cfb7"), "the record follows the conversation its process writes");
    }
}
