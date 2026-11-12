use std::collections::{BTreeMap, BTreeSet};

use crate::error::NorthResult;
use crate::fleet::claims::{self, Rows};
use crate::fleet::launch_queue::{Admission, QueuedLaunch};

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

    /// One pass over the launch queue in order, returning one line per act. A launch whose box another queued launch ahead of it or a session already holds is withdrawn; one `due` names is withdrawn when `gone` says its work is finished and is started otherwise. No launch is begun once `spent` reaches `budget`: those left are counted in one deferred line and wait for the next tick. An unreadable ledger drops nothing for a held box.
    pub fn admit_queued(&self, now_ms: u64, admission: &Admission<'_>) -> NorthResult<Vec<String>> {
        let over = || (admission.spent)() >= admission.budget;
        let queued = self.queued_launches()?;
        let held = if over() { BTreeMap::new() } else { self.live_holders(&queued.iter().flat_map(keys).collect::<Vec<_>>()) };
        let mut ahead: BTreeMap<String, String> = BTreeMap::new();
        let mut lines = Vec::new();
        let mut deferred = 0;
        for entry in &queued {
            let own = keys(entry);
            let blocked = unstartable(entry, &own, &ahead, &held);
            let due = (admission.due)(entry);
            let acts = blocked.is_some() || due;
            let why = if acts && over() {
                deferred += 1;
                blocked
            } else if acts {
                let why = blocked.or_else(|| (admission.gone)(entry));
                match &why {
                    Some(why) => {
                        let released = self.withdraw_queued(&entry.name);
                        lines.push(format!("dropped queued launch {}: {why}", entry.name));
                        if let Err(error) = released {
                            lines.push(format!("queued launch {} was dropped but its claims were not released: {}", entry.name, error.user_message()));
                        }
                    }
                    None => {
                        self.mark_queued_tried(&entry.name, now_ms)?;
                        lines.push(match (admission.start)(entry) {
                            Ok(()) => format!("started queued launch {} at {} priority", entry.name, entry.priority.label()),
                            Err(why) => format!("cannot start queued launch {}: {why}", entry.name),
                        });
                    }
                }
                why
            } else {
                None
            };
            if why.is_none() {
                ahead.extend(own.into_iter().map(|key| (key, entry.name.clone())));
            }
        }
        if deferred > 0 {
            lines.push(format!("deferred {deferred} queued launch{} to the next tick: queue admission spent its {}ms budget", if deferred == 1 { "" } else { "es" }, admission.budget.as_millis()));
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
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::fleet::claims::{Claims, Held};
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::launch_queue::{Admission, Priority, QueuedLaunch};
    use crate::fleet::model::{LaunchRequest, ProviderName};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::registry::fixtures::live;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    fn queued(name: &str, task: &str, claim_box: Option<&str>, created_ms: u64) -> QueuedLaunch {
        let request = LaunchRequest {
            verification: false,
            gates_aside: false,
            container_shell: false,
            container_dir: None,
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
            route: None,
        };
        QueuedLaunch::new(request, Priority::Normal, created_ms, "provider limit".into())
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

        let closed = |queued: &QueuedLaunch| (queued.request.task_ref.as_deref() == Some("north#1")).then(|| "closed on GitHub".to_owned());
        let admitted = fleet.admit_queued(7, &Admission { due: &|_| true, gone: &closed, start: &|_| Ok(()), budget: Duration::MAX, spent: &|| Duration::ZERO }).unwrap();
        let dropped: Vec<&String> = admitted.iter().filter(|line| line.starts_with("dropped")).collect();

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

    /// north#637: at 7:05 AM +07 on 2026-11-11 the laptop's fleet tick was killed at its 50 s start timeout while it dropped about 40 stale queued launches one by one, each a GitHub read and a ledger release, before it started ds-1042-x.
    #[test]
    fn a_tick_admits_queued_launches_only_within_its_budget_and_defers_the_rest() {
        let home = tempfile::tempdir().unwrap();
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), Arc::new(Held::default())).unwrap();
        fleet.queue_launch(&queued("front", "north#100", None, 0)).unwrap();
        for stale in 1..=40 {
            fleet.queue_launch(&queued(&format!("stale-{stale:02}"), &format!("north#{stale}"), None, stale)).unwrap();
        }
        fleet.queue_launch(&queued("ds-1042-x", "north#1042", None, 41)).unwrap();
        let spent = Cell::new(Duration::ZERO);
        let started = RefCell::new(Vec::new());
        let untried = |queued: &QueuedLaunch| queued.tried_ms.is_none();
        let closed = |queued: &QueuedLaunch| {
            spent.set(spent.get() + Duration::from_secs(1));
            queued.name.starts_with("stale").then(|| "closed on GitHub".to_owned())
        };
        let start = |queued: &QueuedLaunch| {
            started.borrow_mut().push(queued.name.clone());
            Ok(())
        };
        let tick = |budget: Duration| {
            spent.set(Duration::ZERO);
            fleet.admit_queued(7, &Admission { due: &untried, gone: &closed, start: &start, budget, spent: &|| spent.get() }).unwrap()
        };

        let first = tick(Duration::from_secs(5));

        assert_eq!(first.len(), 6, "{first:?}");
        assert_eq!(first[0], "started queued launch front at normal priority");
        assert_eq!(first[4], "dropped queued launch stale-04: closed on GitHub");
        assert_eq!(first[5], "deferred 37 queued launches to the next tick: queue admission spent its 5000ms budget");
        assert_eq!(spent.get(), Duration::from_secs(5), "no launch is read once the budget is spent");
        assert_eq!(names(&fleet).len(), 38);

        let second = tick(Duration::from_secs(5));

        assert_eq!(second.first().map(String::as_str), Some("dropped queued launch stale-05: closed on GitHub"), "a launch tried last tick is not read again ahead of the deferred ones");
        assert_eq!(second.last().map(String::as_str), Some("deferred 32 queued launches to the next tick: queue admission spent its 5000ms budget"));

        let rest = tick(Duration::from_secs(60));

        assert_eq!(rest.last().map(String::as_str), Some("started queued launch ds-1042-x at normal priority"));
        assert_eq!(*started.borrow(), ["front", "ds-1042-x"]);
        assert_eq!(names(&fleet), ["front", "ds-1042-x"]);
    }
}
