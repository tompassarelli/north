use std::collections::{BTreeMap, BTreeSet};

use crate::error::NorthResult;
use crate::fleet::claims::{self, Rows};
use crate::fleet::launch_queue::QueuedLaunch;

use super::Fleet;

/// The claim keys a queued launch would take when it starts.
fn keys(queued: &QueuedLaunch) -> Vec<String> {
    let request = &queued.request;
    claims::claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims())
}

/// Why the launch `entry`, whose claim keys are `own`, cannot start: `ahead` names who queued each key before it and `held` who holds each key now.
fn unstartable(entry: &QueuedLaunch, own: &[String], ahead: &BTreeMap<String, String>, held: &BTreeMap<String, String>) -> Option<String> {
    let queued = own.iter().find_map(|key| ahead.get(key).map(|earlier| format!("{key} is already queued by {earlier}")));
    queued.or_else(|| own.iter().find_map(|key| held.get(key).filter(|owner| **owner != entry.name).map(|owner| format!("{owner} holds {key}"))))
}

impl Fleet {
    /// Withdraws the launch queued under `name` and gives up any claim held in that name; false when nothing was queued under it.
    pub fn withdraw_queued(&self, name: &str) -> NorthResult<bool> {
        let withdrawn = self.remove_queued(name)?;
        if withdrawn {
            self.release_all_claims_of(name)?;
        }
        Ok(withdrawn)
    }

    /// Withdraws each queued launch that can no longer start and returns one line per drop: a box another queued launch ahead of it or a session already holds, and, for the launches `due` names, work `gone` says is finished. An unreadable ledger drops nothing for a held box.
    pub fn drop_unstartable(&self, due: &dyn Fn(&QueuedLaunch) -> bool, gone: &dyn Fn(&QueuedLaunch) -> Option<String>) -> NorthResult<Vec<String>> {
        let queued = self.queued_launches()?;
        let wanted: Vec<String> = queued.iter().flat_map(keys).collect();
        let held = self.live_holders(&wanted);
        let mut ahead: BTreeMap<String, String> = BTreeMap::new();
        let mut lines = Vec::new();
        for entry in &queued {
            let own = keys(entry);
            let why = unstartable(entry, &own, &ahead, &held).or_else(|| due(entry).then(|| gone(entry)).flatten());
            let Some(why) = why else {
                ahead.extend(own.into_iter().map(|key| (key, entry.name.clone())));
                continue;
            };
            let released = self.withdraw_queued(&entry.name);
            lines.push(format!("dropped queued launch {}: {why}", entry.name));
            if let Err(error) = released {
                lines.push(format!("queued launch {} was dropped but its claims were not released: {}", entry.name, error.user_message()));
            }
        }
        Ok(lines)
    }

    /// Who holds each of `keys` among the sessions that have not exited, as key to owner; the ledger is asked by issue, so the claims beside a key on its issue are read and left out.
    fn live_holders(&self, keys: &[String]) -> BTreeMap<String, String> {
        let issues: Vec<String> = keys.iter().map(|key| claims::split_key(key).0.to_owned()).collect::<BTreeSet<_>>().into_iter().collect();
        if issues.is_empty() || self.release_exited_claims_on(&issues).is_err() {
            return BTreeMap::new();
        }
        self.claims.holders(Rows::Items(&issues)).unwrap_or_default().into_iter().filter(|(key, _)| keys.contains(key)).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::fleet::claims::{Claims, Held};
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::launch_queue::{Priority, QueuedLaunch};
    use crate::fleet::model::{LaunchRequest, ProviderName};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::registry::fixtures::live;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    fn queued(name: &str, task: &str, claim_box: Option<&str>, created_ms: u64) -> QueuedLaunch {
        let request = LaunchRequest {
            verification: false,
            name: name.into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: "/tmp".into(),
            model: None,
            effort: None,
            prompt: format!("Item: {task}"),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: None,
            task_ref: Some(task.into()),
            claim_box: claim_box.map(str::to_owned),
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
        };
        QueuedLaunch::new(request, Priority::Normal, created_ms, "spend tripwire".into())
    }

    fn names(fleet: &Fleet) -> Vec<String> {
        fleet.queued_launches().unwrap().into_iter().map(|queued| queued.name).collect()
    }

    /// On 2026-11-11 the laptop queue held 44 launches, 39 of them for closed issues, ghost names and boxes queued twice, and neither `north fleet forget` nor `stop` could name one.
    #[test]
    fn a_queued_launch_is_withdrawn_by_forget_and_one_for_a_closed_issue_is_dropped_at_admission() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap();
        let _holder = live(&fleet, "n1-holder");
        claims.hold("north#2@its box", "n1-holder", &BTreeSet::new()).unwrap();
        claims.hold("north#9", "ghost", &BTreeSet::new()).unwrap();
        for entry in [
            queued("ds-1057-x", "north#1", Some("closed box"), 1),
            queued("first", "north#3", Some("same box"), 2),
            queued("second", "north#3", Some("same box"), 3),
            queued("taken", "north#2", Some("its box"), 4),
            queued("ghost", "north#9", None, 5),
            queued("fine", "north#4", Some("open box"), 6),
        ] {
            fleet.queue_launch(&entry).unwrap();
        }

        let dropped = fleet.drop_unstartable(&|_| true, &|queued| (queued.request.task_ref.as_deref() == Some("north#1")).then(|| "closed on GitHub".to_owned())).unwrap();

        assert_eq!(
            dropped,
            [
                "dropped queued launch ds-1057-x: closed on GitHub",
                "dropped queued launch second: north#3@same box is already queued by first",
                "dropped queued launch taken: n1-holder holds north#2@its box",
            ]
        );
        assert_eq!(names(&fleet), ["first", "ghost", "fine"]);

        let forgotten = fleet.forget_unless_queued("ghost", None, None).unwrap();

        assert!(matches!(forgotten.controlled, crate::fleet::registry::Controlled::Withdrawn));
        assert_eq!(names(&fleet), ["first", "fine"]);
        assert!(claims.holders(Rows::Owners(&BTreeSet::from(["ghost".to_owned()]))).unwrap().is_empty(), "a withdrawn launch leaves no claim in its name");
        assert!(fleet.forget_unless_queued("ghost", None, None).is_err(), "a name neither queued nor running is still unknown");
    }
}
