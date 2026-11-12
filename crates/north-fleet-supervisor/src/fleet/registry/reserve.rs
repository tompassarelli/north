//! The operational reserve the policy declares (`[accounts.claude] reserve`): only the proxy, a scheduled routine,
//! the owner's own sessions, the privacy judge, the lander and canaries draw it. A worker or subagent neither starts on
//! it nor continues on it, judged by the account a launch resolves to, never by the name it was given.
use std::path::Path;

use toml_edit::{DocumentMut, Item};

use super::Fleet;
use super::stopping::Halt;
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, Delivery, Phase, Role, SessionRecord};
use crate::fleet::runtime::PROXY;
use crate::fleet::sender::Sender;

/// The account labels the policy at `policy` declares the operational reserve, as a session records them (`api:ORG` or a plan label).
pub fn declared(policy: &Path) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let names = document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("reserve")).and_then(Item::as_array);
    names.into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect()
}

/// Whether a session of `role` under `parent` is a worker or subagent: anything but the proxy that a parent started or that holds a task.
pub fn kept_off(role: Role, parent: Option<&str>) -> bool {
    role != Role::Proxy && (role == Role::Worker || parent.is_some())
}

/// The one-line refusal of a worker or subagent start whose account resolves to a declared reserve, or none; a session a reserve caller's service unit launched (`caller`) is none of those.
pub fn refusal(reserve: &[String], account: &Account, role: Role, parent: Option<&str>, caller: Option<&str>) -> Option<String> {
    let resolved = account.recorded_label();
    (caller.is_none() && kept_off(role, parent) && reserve.iter().any(|held| *held == resolved || *held == account.label))
        .then(|| format!("{resolved} is the operational reserve: no worker or subagent starts on it, whatever account it was launched under; only the proxy, scheduled routines, the privacy judge, the lander and canaries draw it"))
}

impl Fleet {
    pub(super) fn refuse_reserve(&self, record: &SessionRecord, account: &Account) -> NorthResult<()> {
        refusal(&declared(&self.policy), account, record.role(), record.parent.as_deref(), record.reserve_caller.as_deref()).map_or(Ok(()), |line| Err(NorthError::Refused(line)))
    }

    /// Stops every running worker or subagent this host holds on a declared reserve; one line per session stopped or left.
    pub fn park_on_reserve(&self) -> Vec<String> {
        let reserve = declared(&self.policy);
        if reserve.is_empty() {
            return Vec::new();
        }
        let found = self.sessions().unwrap_or_default().into_iter().filter(|record| on_reserve(&reserve, record) && !record.ended() && record.phase() != Some(Phase::Stopped));
        found
            .map(|record| match self.halt(&record.name, Halt::Reserve) {
                Ok(()) => {
                    let line = format!("{}: stopped on {}, the operational reserve no worker or subagent draws; revive it with --account naming another account", record.name, record.account);
                    let told = record.parent.as_deref().unwrap_or(PROXY);
                    match self.send_spooled(&Sender::fleet(), told, &line, Delivery::Queued) {
                        Ok(()) => line,
                        Err(error) => format!("{line}; {told} was not told: {}", error.user_message()),
                    }
                }
                Err(error) => format!("{}: runs on {}, the operational reserve, and was not stopped: {}", record.name, record.account, error.user_message()),
            })
            .collect()
    }
}

/// Whether `record` is a worker or subagent recorded on a declared reserve that no reserve caller launched.
pub fn on_reserve(reserve: &[String], record: &SessionRecord) -> bool {
    record.reserve_caller.is_none() && kept_off(record.role(), record.parent.as_deref()) && reserve.contains(&record.account)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::fleet::model::{ApiKey, LaunchRequest, ProviderName};

    fn account(label: &str, org: Option<&str>) -> Account {
        Account { provider: ProviderName::new("claude"), label: label.to_owned(), home: PathBuf::from("/nonexistent"), token_file: None, api: org.map(|org| ApiKey { org: org.to_owned(), file: PathBuf::from("/nonexistent/key") }) }
    }

    #[test]
    fn a_worker_launched_on_the_pooled_api_account_is_refused_when_the_pool_resolves_it_to_the_reserve() {
        let reserve = vec!["api:fixture-reserve-org".to_owned()];
        let pooled = account("api", Some("fixture-reserve-org"));
        let line = refusal(&reserve, &pooled, Role::Worker, Some("fixture-parent"), None).expect("a worker on the reserve through the pooled name is refused");
        assert!(line.starts_with("api:fixture-reserve-org is the operational reserve"), "{line}");
        assert!(refusal(&reserve, &account("api:fixture-reserve-org", Some("fixture-reserve-org")), Role::Lead, Some("fixture-parent"), None).is_some());
        assert_eq!(refusal(&reserve, &account("api", Some("fixture-other-org")), Role::Worker, Some("fixture-parent"), None), None);
        assert_eq!(refusal(&reserve, &pooled, Role::Proxy, None, None), None);
        assert_eq!(refusal(&reserve, &pooled, Role::Lead, None, None), None);
    }

    fn record(role: &str, parent: Option<&str>, caller: Option<&str>) -> SessionRecord {
        let request: LaunchRequest = serde_json::from_value(serde_json::json!({
            "verification": false, "name": "fixture-session", "provider": "claude", "account": "api", "cwd": "/tmp", "model": null, "prompt": "", "system_prompt": null,
            "foreground_cap_secs": null, "prompt_mode": "north", "idle_exit_minutes": null, "parent": parent, "task_ref": null, "placement": null, "machine": null, "role": role
        }))
        .unwrap();
        let mut record = SessionRecord::launched(&request, None, None, true, 0);
        record.account = "api:fixture-reserve-org".to_owned();
        record.reserve_caller = caller.map(str::to_owned);
        record
    }

    #[test]
    fn a_canary_lander_or_judge_launched_from_its_own_service_unit_runs_on_the_reserve_while_a_worker_found_there_is_parked_and_refused_a_wake() {
        let reserve = vec!["api:fixture-reserve-org".to_owned()];
        let pooled = account("api", Some("fixture-reserve-org"));
        let canary = north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/control.slice/north-runtime-follow.service\n");
        assert_eq!(canary.as_deref(), Some("north-runtime-follow.service"));
        assert!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-land@north.service\n").is_some());
        assert_eq!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-fixture.scope\n"), None);
        assert_eq!(refusal(&reserve, &pooled, Role::Worker, None, canary.as_deref()), None);
        assert!(refusal(&reserve, &pooled, Role::Worker, None, None).is_some());
        assert!(!on_reserve(&reserve, &record("worker", None, canary.as_deref())));
        assert!(!on_reserve(&reserve, &record("lead", Some("fixture-parent"), Some("north-land@north.service"))));
        let parked = record("worker", Some("fixture-parent"), None);
        assert!(on_reserve(&reserve, &parked));
        let relaunched = SessionRecord { model: Some("fixture-model".to_owned()), ..parked };
        assert!(refusal(&reserve, &pooled, relaunched.role(), relaunched.parent.as_deref(), relaunched.reserve_caller.as_deref()).is_some());
    }
}
