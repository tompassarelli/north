//! The spend rules a host start meets on the account it resolves to, never the name it was given
//! (docs/rationale/spend-rule.md): the owner's hold on every API organization refuses it to every caller of every
//! role; the operational reserve the policy declares (`[accounts.claude] reserve`) is drawn only by the proxy, a
//! scheduled routine, the owner's own sessions, the privacy judge, the lander and canaries, so a worker or subagent
//! neither starts on it nor continues on it; and a plan profile whose settings would bill by token starts no session.
use std::path::Path;

use super::Fleet;
use super::stopping::Halt;
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, Delivery, Phase, Role, SessionRecord};
use crate::fleet::runtime::PROXY;
use crate::fleet::sender::Sender;
use crate::spend::hold::{self, Hold};

/// What the policy declares of the accounts a start may not draw: the operational reserve's labels as a session records them (`api:ORG` or a plan label), and the owner's hold on every API organization.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reserve {
    pub labels: Vec<String>,
    pub held: Option<Hold>,
}

impl Reserve {
    /// Why a session recorded on `account` may not run there: the hold's line, else the reserve's.
    pub fn why(&self, account: &str) -> String {
        self.held.as_ref().and_then(|held| held.refusal(account)).unwrap_or_else(|| format!("{account} is the operational reserve no worker or subagent draws"))
    }
}

/// What the policy at `policy` declares; a policy that cannot be read holds every API organization.
pub fn declared(policy: &Path) -> Reserve {
    match hold::read(policy) {
        Ok(document) => Reserve { labels: crate::spend::operational::declared(&document), held: hold::declared(&document) },
        Err(held) => Reserve { labels: Vec::new(), held: Some(held) },
    }
}

/// Whether a session of `role` under `parent` is a worker or subagent: anything but the proxy that a parent started or that holds a task.
pub fn kept_off(role: Role, parent: Option<&str>) -> bool {
    role != Role::Proxy && (role == Role::Worker || parent.is_some())
}

/// The one-line refusal of a start on `account`, or none: under the hold, an API organization for every role and caller; a plan profile whose settings would bill by token; else a worker or subagent whose account resolves to a declared reserve, unless a reserve caller's service unit launched it (`caller`).
pub fn refusal(reserve: &Reserve, account: &Account, role: Role, parent: Option<&str>, caller: Option<&str>) -> Option<String> {
    let resolved = account.recorded_label();
    if let Some(held) = reserve.held.as_ref().filter(|_| account.api.is_some() || hold::api(&resolved)) {
        return Some(held.line(&resolved));
    }
    if account.api.is_none() && account.provider.label() == "claude" && let Some(why) = crate::spend::billing::settings_refusal(&account.home) {
        return Some(format!("{resolved} is refused: {why}"));
    }
    (caller.is_none() && kept_off(role, parent) && reserve.labels.iter().any(|held| *held == resolved || *held == account.label))
        .then(|| format!("{resolved} is the operational reserve: no worker or subagent starts on it, whatever account it was launched under; only the proxy, scheduled routines, the privacy judge, the lander and canaries draw it"))
}

/// The reserve caller a host start draws as: the one `record` was stamped with, only while the process starting it runs in that same unit, so a revive or relaunch from anywhere else meets the rule afresh.
pub fn standing<'a>(stamp: Option<&'a str>, starter: Option<&str>) -> Option<&'a str> {
    stamp.filter(|stamp| starter == Some(*stamp))
}

impl Fleet {
    pub(super) fn refuse_reserve(&self, record: &SessionRecord, account: &Account) -> NorthResult<()> {
        let caller = north_base::platform::session_scope::reserve_caller();
        refusal(&declared(&self.policy), account, record.role(), record.parent.as_deref(), standing(record.reserve_caller.as_deref(), caller.as_deref())).map_or(Ok(()), |line| Err(NorthError::Refused(line)))
    }

    /// Stops every running session this host holds on a held API organization, and every worker or subagent on a declared reserve; one line per session stopped or left.
    pub fn park_on_reserve(&self) -> Vec<String> {
        let reserve = declared(&self.policy);
        if reserve.labels.is_empty() && reserve.held.is_none() {
            return Vec::new();
        }
        let found = self.sessions().unwrap_or_default().into_iter().filter(|record| on_reserve(&reserve, record) && !record.ended() && record.phase() != Some(Phase::Stopped));
        found
            .map(|record| match self.halt(&record.name, Halt::Reserve) {
                Ok(()) => {
                    let line = format!("{}: stopped: {}; revive it with --account naming a plan account", record.name, reserve.why(&record.account));
                    let told = record.parent.as_deref().unwrap_or(PROXY);
                    match self.send_spooled(&Sender::fleet(), told, &line, Delivery::Queued) {
                        Ok(()) => line,
                        Err(error) => format!("{line}; {told} was not told: {}", error.user_message()),
                    }
                }
                Err(error) => format!("{}: runs on {}, which it may not draw, and was not stopped: {}", record.name, record.account, error.user_message()),
            })
            .collect()
    }
}

/// Whether `record` runs on an account it may not draw: any API organization under the hold, whatever its role, or a declared reserve as a worker or subagent no reserve caller launched.
pub fn on_reserve(reserve: &Reserve, record: &SessionRecord) -> bool {
    (reserve.held.is_some() && hold::api(&record.account)) || (record.reserve_caller.is_none() && kept_off(record.role(), record.parent.as_deref()) && reserve.labels.contains(&record.account))
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
        let reserve = Reserve { labels: vec!["api:fixture-reserve-org".to_owned()], held: None };
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
    fn a_canary_launched_from_the_promotion_unit_runs_on_the_reserve_while_an_owner_worker_from_a_lander_unit_or_a_revive_elsewhere_is_refused() {
        let reserve = Reserve { labels: vec!["api:fixture-reserve-org".to_owned()], held: None };
        let pooled = account("api", Some("fixture-reserve-org"));
        let canary = north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/control.slice/north-runtime-follow.service\n");
        assert_eq!(canary.as_deref(), Some("north-runtime-follow.service"));
        assert_eq!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-land@north.service\n"), None);
        assert_eq!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-fixture.scope\n"), None);
        assert_eq!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-runtime-canary-4242.scope\n").as_deref(), Some("north-runtime-canary-4242.scope"));
        assert_eq!(north_base::platform::session_scope::reserve_caller_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-runtime-canary-.scope\n"), None);
        assert_eq!(refusal(&reserve, &pooled, Role::Worker, None, canary.as_deref()), None);
        assert!(refusal(&reserve, &pooled, Role::Worker, None, None).is_some());
        assert!(!on_reserve(&reserve, &record("worker", None, canary.as_deref())));
        assert!(!on_reserve(&reserve, &record("lead", Some("fixture-parent"), canary.as_deref())));
        let parked = record("worker", Some("fixture-parent"), None);
        assert!(on_reserve(&reserve, &parked));
        let relaunched = SessionRecord { model: Some("fixture-model".to_owned()), ..parked };
        assert!(refusal(&reserve, &pooled, relaunched.role(), relaunched.parent.as_deref(), relaunched.reserve_caller.as_deref()).is_some());
        let stamped = record("worker", None, canary.as_deref());
        assert_eq!(standing(stamped.reserve_caller.as_deref(), None), None);
        assert!(refusal(&reserve, &pooled, stamped.role(), None, standing(stamped.reserve_caller.as_deref(), Some("north-land@north.service"))).is_some());
        assert_eq!(standing(stamped.reserve_caller.as_deref(), canary.as_deref()), canary.as_deref());
    }
}
