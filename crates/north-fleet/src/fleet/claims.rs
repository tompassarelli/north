//! The threads claim a fleet session holds on its task while its host runs, so a hand launch is as visible to dispatch as a dispatched one.
use std::collections::BTreeSet;

use crate::error::{NorthError, NorthResult};

use super::model::SessionRecord;

/// Which claim rows a read asks the ledger for: those on these items, or those these owners hold.
#[derive(Clone, Copy, Debug)]
pub enum Rows<'a> {
    Items(&'a [String]),
    Owners(&'a BTreeSet<String>),
}

/// What a claim key carries between an issue and the text of the box it covers, as `north#9@the box`; a key without it covers the whole issue.
const BOX_MARK: char = '@';

/// The key a launch's claim on `task` has: the task alone, or the box of it `box_text` names.
pub fn claim_key(task: &str, box_text: Option<&str>) -> String {
    box_text.map_or_else(|| task.to_owned(), |text| format!("{task}{BOX_MARK}{text}"))
}

/// The issue and the box text a claim key names.
pub fn split_key(key: &str) -> (&str, Option<&str>) {
    key.split_once(BOX_MARK).map_or((key, None), |(task, text)| (task, Some(text)))
}

/// The keys of the claims a launch of `tasks` makes: the box applies to the first task, whose issue it is a box of, and `others` are the boxes its brief names of other issues.
pub fn claim_keys(tasks: &[String], box_text: Option<&str>, others: &[String]) -> Vec<String> {
    tasks.iter().enumerate().map(|(index, task)| claim_key(task, box_text.filter(|_| index == 0))).chain(others.iter().cloned()).collect()
}

pub trait Claims: Send + Sync {
    /// Claims the claim key `item` for `session`; a claim a live owner holds is `NorthError::Refused`, while one an exited owner left or one past its ETA moves to `session`, and Some says from whom and why.
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>>;
    /// Gives the claim up only while `session` still owns it, so a later owner's claim stays.
    fn release(&self, item: &str, session: &str) -> NorthResult<()>;
    /// The claim rows `rows` names as (item, owner), including those no dispatch or launch honours any more.
    fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>>;
}

/// Claims every task before the session starts, so a live holder refuses the launch and nothing is left half-claimed; a ledger that cannot be read waits for the claim `hold_for` makes once the session runs.
pub fn hold_all(claims: &dyn Claims, tasks: &[String], session: &str) -> NorthResult<()> {
    for (index, task) in tasks.iter().enumerate() {
        match claims.hold(task, session) {
            Err(error @ NorthError::Refused(_)) => {
                for held in &tasks[..index] {
                    let _ = claims.release(held, session);
                }
                return Err(error);
            }
            Err(error) => eprintln!("north fleet: {session} does not hold {task} yet: {}", error.user_message()),
            Ok(Some(moved)) => eprintln!("north fleet: {moved}"),
            Ok(None) => {}
        }
    }
    Ok(())
}

/// The tasks a session claims for itself: each one no session above it already holds, so a lead holding an issue keeps
/// its claim while the children it staffs on that issue's boxes work under it; an unread ledger leaves every task to the claim.
pub fn own_tasks(claims: &dyn Claims, tasks: &[String], ancestors: &BTreeSet<String>) -> Vec<String> {
    if ancestors.is_empty() || tasks.is_empty() {
        return tasks.to_vec();
    }
    let issues: Vec<String> = tasks.iter().map(|task| split_key(task).0.to_owned()).collect();
    let above: BTreeSet<String> = claims.holders(Rows::Items(&issues)).unwrap_or_default().into_iter().filter(|(_, owner)| ancestors.contains(owner)).map(|(key, _)| split_key(&key).0.to_owned()).collect();
    tasks.iter().filter(|task| !above.contains(split_key(task).0)).cloned().collect()
}

pub fn hold_for(claims: &dyn Claims, record: &SessionRecord, ancestors: &BTreeSet<String>) {
    if record.verification {
        return;
    }
    for task in own_tasks(claims, &record.claim_keys(), ancestors) {
        if let Err(error) = claims.hold(&task, &record.name) {
            eprintln!("north fleet: {} does not hold {task}: {}", record.name, error.user_message());
        }
    }
}

/// What an exited sweep cleared, and the owners whose claim it could not give up, so they are swept again.
#[derive(Debug, Default)]
pub struct Swept {
    pub released: usize,
    pub unreleased: BTreeSet<String>,
}

/// Gives up every claim among `rows` whose owner is in `exited`, whatever its standing: a session that exited holds nothing, and the ledger keeps its row until someone clears it.
pub fn release_exited(claims: &dyn Claims, exited: &BTreeSet<String>, rows: Rows<'_>) -> NorthResult<Swept> {
    let mut swept = Swept::default();
    for (item, owner) in claims.holders(rows)? {
        if !exited.contains(&owner) {
            continue;
        }
        match claims.release(&item, &owner) {
            Ok(()) => swept.released += 1,
            Err(error) => {
                eprintln!("north fleet: {owner} did not release {item}: {}", error.user_message());
                swept.unreleased.insert(owner);
            }
        }
    }
    Ok(swept)
}

pub fn release_for(claims: &dyn Claims, record: &SessionRecord) {
    if record.verification {
        return;
    }
    for task in record.claim_keys() {
        if let Err(error) = claims.release(&task, &record.name) {
            eprintln!("north fleet: {} did not release {task}: {}", record.name, error.user_message());
        }
    }
}

#[derive(Default)]
pub struct Held(pub std::sync::Mutex<Vec<(&'static str, String, String)>>);

impl Claims for Held {
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
        self.0.lock().unwrap().push(("hold", item.to_owned(), session.to_owned()));
        Ok(None)
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
        fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
            if item == self.0 {
                return Err(NorthError::Refused(format!("{item} is already owned by tom")));
            }
            self.1.lock().unwrap().push(format!("hold {item} {session}"));
            Ok(None)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.1.lock().unwrap().push(format!("release {item} {session}"));
            Ok(())
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_launch_naming_an_item_a_live_session_holds_is_refused_and_leaves_no_claim_behind() {
        let claims = Owned("north#9", Mutex::default());
        let tasks = ["north#7".to_owned(), "north#9".to_owned()];
        let error = hold_all(&claims, &tasks, "worker").unwrap_err();
        assert!(matches!(error, NorthError::Refused(why) if why == "north#9 is already owned by tom"));
        assert_eq!(*claims.1.lock().unwrap(), ["hold north#7 worker", "release north#7 worker"]);
    }

    #[test]
    fn children_a_lead_staffs_on_boxes_of_its_own_issue_start_and_leave_the_lead_holding_it() {
        let claims = Held::default();
        claims.hold("north#757", "n-state-machines").unwrap();
        let tasks = ["north#757".to_owned()];
        let lead = BTreeSet::from(["n-state-machines".to_owned()]);
        for child in ["gex45-n757-gates", "gex45-n757-layers"] {
            hold_all(&claims, &own_tasks(&claims, &tasks, &lead), child).unwrap();
        }
        assert_eq!(claims.holders(Rows::Items(&tasks)).unwrap(), [("north#757".to_owned(), "n-state-machines".to_owned())]);
        assert_eq!(own_tasks(&claims, &tasks, &BTreeSet::from(["proxy".to_owned()])), tasks, "a launch outside the lead's tree still claims, so the live holder refuses it");
    }

    #[test]
    fn a_launch_claims_the_box_it_names_on_its_first_task_and_a_lead_holding_the_whole_issue_covers_it() {
        let tasks = ["north#782".to_owned(), "north#9".to_owned()];
        let keys = claim_keys(&tasks, Some("the staffing replays"), &[]);
        assert_eq!(keys, ["north#782@the staffing replays", "north#9"]);
        assert_eq!(split_key(&keys[0]), ("north#782", Some("the staffing replays")));
        let claims = Held::default();
        claims.hold("north#782", "n-lead").unwrap();
        let lead = BTreeSet::from(["n-lead".to_owned()]);
        assert_eq!(own_tasks(&claims, &keys, &lead), ["north#9"], "the lead's whole claim covers the box its child works");
        assert_eq!(own_tasks(&claims, &keys, &BTreeSet::new()), keys);
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
