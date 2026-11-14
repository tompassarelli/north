//! The threads claim a fleet session holds on its task while its host runs, so a hand launch is as visible to dispatch as a dispatched one.
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};

use super::model::SessionRecord;

/// Which claim rows a read asks the ledger for: those on these items, or those these owners hold.
#[derive(Clone, Copy, Debug)]
pub enum Rows<'a> {
    Items(&'a [String]),
    Owners(&'a BTreeSet<String>),
}

pub use north_fleet_store::claim_key::{claim_key, claim_keys, split_key};

/// How long a launch waits for a claim the ledger only queued to land, a few times the 4 to 5 s a loaded ledger host took to answer on 2026-11-06.
const QUEUED_CLAIM_WAIT: Duration = Duration::from_secs(30);

/// How long a launch waits between two reads of a queued claim.
const QUEUED_CLAIM_POLL: Duration = Duration::from_millis(500);

/// What the ledger did with a claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Taken {
    /// The ledger recorded the claim.
    Held,
    /// The ledger recorded the claim after moving it from an earlier owner, and says from whom and why.
    Moved(String),
    /// The ledger did not answer in time and the claim waits in a spool, under this name, to land once it does, so no other launch sees it yet.
    Queued(String),
}

pub trait Claims: Send + Sync {
    /// Claims the claim key `item` for `session`; a claim a live owner holds is `NorthError::Refused`, while one an exited owner left or one past its ETA moves to `session`. A box claim beside a live whole-issue claim is refused unless that claim's owner is one of `ancestors`, the sessions above `session`.
    fn hold(&self, item: &str, session: &str, ancestors: &BTreeSet<String>) -> NorthResult<Taken>;
    /// How long a launch waits for a claim [`Taken::Queued`] to land before it refuses.
    fn queued_claim_wait(&self) -> Duration {
        QUEUED_CLAIM_WAIT
    }
    /// What the ledger said when it refused the claim queued under `queued`; `None` while it waits or once it landed.
    fn refusal(&self, _queued: &str) -> Option<String> {
        None
    }
    /// Withdraws the claims queued for `session` that the ledger has not been sent, so they never land; returns how many.
    fn withdraw_queued(&self, _session: &str) -> usize {
        0
    }
    /// Gives the claim up only while `session` still owns it, so a later owner's claim stays.
    fn release(&self, item: &str, session: &str) -> NorthResult<()>;
    /// The claim rows `rows` names as (item, owner), including those no dispatch or launch honours any more.
    fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>>;
    /// Whether the key `held`, as [`Claims::holders`] spells it, is the claim key `key` a launch asked for.
    fn covers(&self, held: &str, key: &str) -> bool {
        held == key
    }
}

/// Claims every task before the session starts, so a live holder refuses the launch and nothing is left half-claimed; a ledger that cannot be read waits for the claim `hold_for` makes once the session runs. A claim the ledger only queued is waited for until it lands, because a second launch of the same item sees no holder before then; one that does not land in time refuses the launch.
pub fn hold_all(claims: &dyn Claims, tasks: &[String], session: &str, ancestors: &BTreeSet<String>) -> NorthResult<()> {
    let mut queued = Vec::new();
    for (index, task) in tasks.iter().enumerate() {
        match claims.hold(task, session, ancestors) {
            Err(error @ NorthError::Refused(_)) => {
                give_up(claims, &tasks[..index], session);
                return Err(error);
            }
            Err(error) => eprintln!("north fleet: {session} does not hold {task} yet: {}", error.user_message()),
            Ok(Taken::Moved(moved)) => eprintln!("north fleet: {moved}"),
            Ok(Taken::Queued(spooled)) => queued.push((task.clone(), spooled)),
            Ok(Taken::Held) => {}
        }
    }
    if let Err(error) = land_queued(claims, &queued, session) {
        give_up(claims, tasks, session);
        return Err(error);
    }
    Ok(())
}

fn give_up(claims: &dyn Claims, tasks: &[String], session: &str) {
    claims.withdraw_queued(session);
    for held in tasks {
        let _ = claims.release(held, session);
    }
}

/// Reads the ledger until every key in `queued` is held by `session`: a key the ledger refused once the spool sent it refuses with the ledger's own words, a key another owner holds refuses, and keys a ledger that answers still does not show when the claim's wait ends refuse too. A ledger that does not answer a read leaves the claim queued, to land once the host reaches it, and the launch starts.
fn land_queued(claims: &dyn Claims, waiting: &[(String, String)], session: &str) -> NorthResult<()> {
    if waiting.is_empty() {
        return Ok(());
    }
    let queued: Vec<&str> = waiting.iter().map(|(key, _)| key.as_str()).collect();
    let issues: Vec<String> = queued.iter().map(|key| split_key(key).0.to_owned()).collect();
    let deadline = Instant::now() + claims.queued_claim_wait();
    loop {
        if let Some((key, why)) = waiting.iter().find_map(|(key, spooled)| Some((key, claims.refusal(spooled)?))) {
            return Err(NorthError::Refused(format!("the threads ledger refused the claim of {key}: {why}")));
        }
        let Ok(rows) = claims.holders(Rows::Items(&issues)) else {
            eprintln!("north fleet: the threads ledger did not answer, so {session} starts with {} queued", queued.join(", "));
            return Ok(());
        };
        if let Some(owner) = rows.iter().find(|(held, owner)| owner != session && queued.iter().any(|key| claims.covers(held, key))).map(|(_, owner)| owner) {
            return Err(NorthError::Refused(format!("{} is already owned by {owner}", split_key(queued[0]).0)));
        }
        if queued.iter().all(|key| rows.iter().any(|(held, owner)| owner == session && claims.covers(held, key))) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(NorthError::State(format!("the claim of {} is only queued: the threads ledger did not take it within {} s, so {session} does not start beside a holder it cannot see", queued.join(", "), claims.queued_claim_wait().as_secs())));
        }
        std::thread::sleep(QUEUED_CLAIM_POLL);
    }
}

/// The tasks a session claims for itself: it leaves a whole-issue claim to a session above it, so a lead keeps the
/// issue while its children staff its boxes, but it always attempts a box claim, which the ledger admits under the
/// lead's claim and refuses beside anyone else's; an unread ledger leaves every task to the claim.
pub fn own_tasks(claims: &dyn Claims, tasks: &[String], ancestors: &BTreeSet<String>) -> Vec<String> {
    if ancestors.is_empty() || tasks.is_empty() {
        return tasks.to_vec();
    }
    let issues: Vec<String> = tasks.iter().map(|task| split_key(task).0.to_owned()).collect();
    let above: BTreeSet<String> = claims.holders(Rows::Items(&issues)).unwrap_or_default().into_iter().filter(|(_, owner)| ancestors.contains(owner)).map(|(key, _)| split_key(&key).0.to_owned()).collect();
    tasks.iter().filter(|task| split_key(task).1.is_some() || !above.contains(split_key(task).0)).cloned().collect()
}

pub fn hold_for(claims: &dyn Claims, record: &SessionRecord, ancestors: &BTreeSet<String>) {
    if record.verification {
        return;
    }
    for task in own_tasks(claims, &record.claim_keys(), ancestors) {
        if let Err(error) = claims.hold(&task, &record.name, ancestors) {
            eprintln!("north fleet: {} does not hold {task}: {}", record.name, error.user_message());
        }
    }
}

/// Gives up every claim `session` holds on `task`'s issue, its boxes as well as the whole issue, so once the record drops it too dispatch staffs the issue; returns how many it gave up.
pub fn release_task(claims: &dyn Claims, session: &str, task: &str) -> NorthResult<usize> {
    let issues = [split_key(task).0.to_owned()];
    let held: Vec<String> = claims.holders(Rows::Items(&issues))?.into_iter().filter(|(_, owner)| owner == session).map(|(key, _)| key).collect();
    for key in &held {
        claims.release(key, session)?;
    }
    Ok(held.len())
}

pub fn release_for(claims: &dyn Claims, record: &SessionRecord) {
    if record.verification {
        return;
    }
    claims.withdraw_queued(&record.name);
    for task in record.claim_keys() {
        if let Err(error) = claims.release(&task, &record.name) {
            eprintln!("north fleet: {} did not release {task}: {}", record.name, error.user_message());
        }
    }
}

#[derive(Default)]
pub struct Held(pub std::sync::Mutex<Vec<(&'static str, String, String)>>);

impl Claims for Held {
    fn hold(&self, item: &str, session: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
        self.0.lock().unwrap().push(("hold", item.to_owned(), session.to_owned()));
        Ok(Taken::Held)
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        self.0.lock().unwrap().push(("release", item.to_owned(), session.to_owned()));
        Ok(())
    }

    fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
        let mut held: Vec<(String, String)> = Vec::new();
        for (kind, item, session) in self.0.lock().unwrap().iter() {
            if *kind == "hold" {
                held.retain(|(held, _)| held != item);
                held.push((item.clone(), session.clone()));
            } else {
                held.retain(|(held, owner)| !(held == item && owner == session));
            }
        }
        held.retain(|(item, owner)| match rows {
            Rows::Items(items) => items.iter().any(|issue| issue == split_key(item).0),
            Rows::Owners(owners) => owners.contains(owner),
        });
        Ok(held)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Owned<'a>(&'a str, Mutex<Vec<String>>);

    impl Claims for Owned<'_> {
        fn hold(&self, item: &str, session: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            if item == self.0 {
                return Err(NorthError::Refused(format!("{item} is already owned by tom")));
            }
            self.1.lock().unwrap().push(format!("hold {item} {session}"));
            Ok(Taken::Held)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.1.lock().unwrap().push(format!("release {item} {session}"));
            Ok(())
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(Vec::new())
        }
    }

    /// A ledger with the box rule: a whole-issue claim refuses a box claim beside it unless its owner is above the
    /// claimant, and records every claim it admits.
    #[derive(Default)]
    struct Beside(Mutex<Vec<(String, String)>>);

    impl Claims for Beside {
        fn hold(&self, item: &str, session: &str, ancestors: &BTreeSet<String>) -> NorthResult<Taken> {
            let (issue, boxed) = split_key(item);
            let mut held = self.0.lock().unwrap();
            let outside = held.iter().find(|(claim, owner)| {
                owner.as_str() != session && split_key(claim).1.is_none() && split_key(claim).0 == issue && !ancestors.contains(owner)
            });
            if let (true, Some((_, owner))) = (boxed.is_some(), outside) {
                return Err(NorthError::Refused(format!("{issue} is already owned by {owner}")));
            }
            held.retain(|(claim, owner)| !(claim.as_str() == item && owner.as_str() == session));
            held.push((item.to_owned(), session.to_owned()));
            Ok(Taken::Held)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.0.lock().unwrap().retain(|(claim, owner)| !(claim.as_str() == item && owner.as_str() == session));
            Ok(())
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(claim, owner)| match rows {
                    Rows::Items(items) => items.iter().any(|item| item == split_key(claim).0),
                    Rows::Owners(owners) => owners.contains(owner),
                })
                .cloned()
                .collect())
        }
    }

    /// A ledger that only queues a claim, and whose reads give no answer or answer with these rows.
    struct Queues(Option<&'static [(&'static str, &'static str)]>);

    impl Claims for Queues {
        fn hold(&self, _: &str, _: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            Ok(Taken::Queued("spooled".to_owned()))
        }

        fn queued_claim_wait(&self) -> Duration {
            Duration::from_millis(100)
        }

        fn release(&self, _: &str, _: &str) -> NorthResult<()> {
            Ok(())
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            let rows = self.0.ok_or_else(|| NorthError::State("the threads ledger did not answer".into()))?;
            Ok(rows.iter().map(|(item, owner)| ((*item).to_owned(), (*owner).to_owned())).collect())
        }
    }

    /// On 2026-11-11 every launch on gex45, cpx62 and cpx62b was refused for 30 s of unanswered reads, because a build host cannot reach the ledger on nexus within the bound, also when the laptop already held the box under the launch's name.
    #[test]
    fn a_launch_on_a_host_that_cannot_reach_the_ledger_starts_when_its_claim_is_held_or_queued() {
        let tasks = ["north#1109".to_owned()];
        hold_all(&Queues(None), &tasks, "cpx62-worker", &BTreeSet::new()).unwrap();
        hold_all(&Queues(Some(&[("north#1109", "cpx62-worker")])), &tasks, "cpx62-worker", &BTreeSet::new()).unwrap();

        let beside = hold_all(&Queues(Some(&[("north#1109", "laptop-lead")])), &tasks, "cpx62-worker", &BTreeSet::new()).unwrap_err();
        assert!(matches!(&beside, NorthError::Refused(why) if why == "north#1109 is already owned by laptop-lead"), "{}", beside.user_message());
        let unseen = hold_all(&Queues(Some(&[("north#1110", "cpx62-worker")])), &tasks, "cpx62-worker", &BTreeSet::new()).unwrap_err();
        assert!(matches!(&unseen, NorthError::State(why) if why.contains("is only queued")), "{}", unseen.user_message());
    }

    /// A ledger that queues a claim and, once the spool sends it, refuses it.
    #[derive(Default)]
    struct RefusesQueued(Mutex<Vec<String>>);

    impl Claims for RefusesQueued {
        fn hold(&self, _: &str, _: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            Ok(Taken::Queued("1794399091624627536-3178642.argv".to_owned()))
        }

        fn refusal(&self, queued: &str) -> Option<String> {
            (queued == "1794399091624627536-3178642.argv").then(|| "the ledger host exited 1: NorthThreads: --box an-ordered-promotion-of-a-landed-commit names no box of north#1144".to_owned())
        }

        fn withdraw_queued(&self, session: &str) -> usize {
            self.0.lock().unwrap().push(session.to_owned());
            1
        }

        fn release(&self, _: &str, _: &str) -> NorthResult<()> {
            Ok(())
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(Vec::new())
        }
    }

    /// On 2026-11-11 the ledger refused f-promote-order's queued claim 3 s after the spool sent it, and the launch polled for 30 s and then blamed the ledger for not taking it. On 2026-11-14 a launch that was refused after it queued a box claim left the claim in the spool, and it landed later and took the box from the worker that held it.
    #[test]
    fn a_queued_claim_the_ledger_refuses_refuses_the_launch_at_once_and_a_refused_launch_withdraws_its_queued_claims() {
        let ledger = RefusesQueued::default();
        let refused = hold_all(&ledger, &["north#1144@an_ordered_promotion_of_a_landed_commit".to_owned()], "f-promote-order", &BTreeSet::new()).unwrap_err();
        assert!(matches!(&refused, NorthError::Refused(why) if why.contains("names no box of north#1144")), "{}", refused.user_message());
        assert_eq!(*ledger.0.lock().unwrap(), ["f-promote-order"], "the refused launch left its queued claim in the spool");

        let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "f-start-refused", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": null, "task_ref": "north#1144"})).unwrap();
        release_for(&ledger, &record);
        assert_eq!(ledger.0.lock().unwrap().last().map(String::as_str), Some("f-start-refused"), "a launch that fails after it began withdraws its queued claim too");
    }

    #[test]
    fn a_launch_naming_an_item_a_live_session_holds_is_refused_and_leaves_no_claim_behind() {
        let claims = Owned("north#9", Mutex::default());
        let tasks = ["north#7".to_owned(), "north#9".to_owned()];
        let error = hold_all(&claims, &tasks, "worker", &BTreeSet::new()).unwrap_err();
        assert!(matches!(error, NorthError::Refused(why) if why == "north#9 is already owned by tom"));
        assert_eq!(*claims.1.lock().unwrap(), ["hold north#7 worker", "release north#7 worker"]);
    }

    #[test]
    fn children_a_lead_staffs_on_boxes_of_its_own_issue_start_and_leave_the_lead_holding_it() {
        let claims = Held::default();
        claims.hold("north#757", "n-state-machines", &BTreeSet::new()).unwrap();
        let tasks = ["north#757".to_owned()];
        let lead = BTreeSet::from(["n-state-machines".to_owned()]);
        for child in ["gex45-n757-gates", "gex45-n757-layers"] {
            hold_all(&claims, &own_tasks(&claims, &tasks, &lead), child, &lead).unwrap();
        }
        assert_eq!(claims.holders(Rows::Items(&tasks)).unwrap(), [("north#757".to_owned(), "n-state-machines".to_owned())]);
        assert_eq!(own_tasks(&claims, &tasks, &BTreeSet::from(["proxy".to_owned()])), tasks, "a launch outside the lead's tree still claims, so the live holder refuses it");
    }

    #[test]
    fn a_launch_claims_the_box_it_names_on_its_first_task_even_beneath_a_lead_holding_the_whole_issue() {
        let tasks = ["north#782".to_owned(), "north#9".to_owned()];
        let keys = claim_keys(&tasks, Some("the staffing replays"), &[]);
        assert_eq!(keys, ["north#782@the staffing replays", "north#9"]);
        assert_eq!(split_key(&keys[0]), ("north#782", Some("the staffing replays")));
        let claims = Beside::default();
        claims.hold("north#782", "n-lead", &BTreeSet::new()).unwrap();
        let lead = BTreeSet::from(["n-lead".to_owned()]);
        assert_eq!(own_tasks(&claims, &["north#782".to_owned()], &lead), Vec::<String>::new(), "the lead keeps the whole issue");
        assert_eq!(own_tasks(&claims, &keys, &lead), keys, "the child claims the box under the lead, which the ledger admits");
        hold_all(&claims, &own_tasks(&claims, &keys[..1], &lead), "n-924-child", &lead).unwrap();
        assert_eq!(claims.holders(Rows::Owners(&BTreeSet::from(["n-924-child".to_owned()]))).unwrap(), [(keys[0].clone(), "n-924-child".to_owned())]);

        let outside = BTreeSet::new();
        let error = hold_all(&claims, &own_tasks(&claims, &keys[..1], &outside), "n-924-outsider", &outside).unwrap_err();
        assert!(matches!(error, NorthError::Refused(why) if why == "north#782 is already owned by n-lead"));
        assert_eq!(own_tasks(&claims, &keys, &outside), keys);
    }

    /// On 2026-11-10 the proxy held north#722 with `north fleet hold`, and once its cause was fixed nothing could drop it: dispatch kept skipping it as held by the proxy.
    #[test]
    fn a_released_hold_empties_the_sessions_row_and_its_claim_so_dispatch_staffs_the_issue() {
        let mut record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "proxy", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": null})).unwrap();
        let claims = Held::default();
        let task = "north#722";
        assert!(record.hold(task));
        claims.hold(task, "proxy", &BTreeSet::new()).unwrap();
        claims.hold("north#722@a box", "proxy", &BTreeSet::new()).unwrap();

        assert!(record.release(task));
        assert_eq!(release_task(&claims, "proxy", task).unwrap(), 2);

        assert_eq!(record.tasks().count(), 0, "the row still listed {task}");
        assert!(record.claim_keys().is_empty());
        assert_eq!(claims.holders(Rows::Items(&[task.to_owned()])).unwrap(), Vec::<(String, String)>::new(), "dispatch still read {task} as held");
    }

    #[test]
    fn a_brief_naming_another_issues_box_claims_that_box_too() {
        let brief = "Item: north#882 Graph context in dispatched briefs\nBox: Graph context in dispatched briefs: dispatch forwards the declared Files\nBox: north#873 Arm: dispatch and `north fleet launch` draw `Arm: context-pack`\nItem: north#880 as-of views\nItem: north#879\n";
        let others = crate::fleet::model::other_box_claims(brief);
        assert_eq!(others, ["north#873@Arm: dispatch and `north fleet launch` draw `Arm: context-pack`", "north#880@as-of views"]);
        let keys = claim_keys(&["north#882".to_owned()], Some("Graph context in dispatched briefs"), &others);
        assert_eq!(keys.len(), 3);
        assert_eq!(split_key(&keys[1]).0, "north#873");
        assert!(crate::fleet::model::other_box_claims("Item: north#888 a box\nBox: the box\n").is_empty(), "a brief that names only its own box claims nothing more");
    }
}
