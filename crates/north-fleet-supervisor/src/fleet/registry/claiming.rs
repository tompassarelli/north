use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::claims::{self, Rows};
use crate::fleet::model::{Account, LaunchRequest, SessionRecord, Status};
use crate::fleet::records::{CLAIMS_RELEASED, CLAIMS_SWEEP_LOG, read_record, write_record};
use crate::fleet::runtime::Opening;

use super::{Fleet, session_running};

/// How many owners one `threads holder` read names, so a fleet with hundreds of exited sessions never asks for them all in a read the ledger cannot answer within its bound.
const OWNERS_PER_READ: usize = 64;

impl Fleet {
    /// Claims every task of a launch before its session starts: a live holder refuses it. A `--test` session claims nothing.
    pub(super) fn claim_before_start(&self, request: &LaunchRequest, tasks: &[String], verification: bool) -> NorthResult<()> {
        if verification {
            return Ok(());
        }
        claims::hold_all(self.claims.as_ref(), &claims::own_tasks(self.claims.as_ref(), tasks, &self.ancestors(request.parent.as_deref())), &request.name)
    }

    /// Starts a fresh session, giving its claims back when it does not start.
    pub(super) fn start_claiming(&self, record: &mut SessionRecord, account: &Account, opening: Opening) -> NorthResult<()> {
        let started = self.start(record, account, opening);
        if started.is_err() {
            claims::release_for(self.claims.as_ref(), record);
        }
        started
    }
}

impl Fleet {
    /// Adds `task` to the issues the running session `name` holds, as a launch's `--task` would have: the session's record names it, so `north fleet list --json` shows it to every dispatch whatever the threads ledger says, and its claim in the ledger is taken on top. A ledger that cannot be read is a warning, never a failure, because the record already holds the issue.
    pub fn hold_task(&self, name: &str, task: &str) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        if record.verification {
            return Ok(());
        }
        if !record.tasks().any(|held| held == task) {
            if record.task_ref.is_none() {
                record.task_ref = Some(task.to_owned());
            } else {
                record.extra_tasks.push(task.to_owned());
            }
            write_record(&session_dir, &record)?;
        }
        match self.claims.hold(task, name) {
            Ok(Some(moved)) => println!("{moved}"),
            Ok(None) => println!("{name} holds {task}"),
            Err(error) => eprintln!("north fleet: {name} holds {task} in its record, but the threads ledger did not take its claim: {}", error.user_message()),
        }
        Ok(())
    }

    /// Clears every claim an exited session left in the ledger, whichever way it exited, so dispatch, launch and `threads show` stop naming it; a parked, idle or working session keeps its claims. Each owner is swept once and marked, so a later sweep asks the ledger only about sessions that exited since, in reads of at most [`OWNERS_PER_READ`] owners; a read that fails keeps the marks of the reads before it. Each read asks only about sessions still exited when it starts, and marks only those still exited when it ends, so a session revived during a long sweep keeps its claims and is swept when it exits again. Returns how many it cleared.
    pub fn release_exited_claims(&self) -> NorthResult<usize> {
        let unswept = self.unswept()?;
        let mut released = 0;
        for chunk in unswept.chunks(OWNERS_PER_READ) {
            let exited = self.exited()?;
            let owners: BTreeSet<String> = chunk.iter().filter(|name| exited.contains(*name)).cloned().collect();
            if owners.is_empty() {
                continue;
            }
            let swept = claims::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
            released += swept.released;
            let exited = self.exited()?;
            for owner in owners.difference(&swept.unreleased).filter(|owner| exited.contains(*owner)) {
                fs::write(self.root.join(owner).join(CLAIMS_RELEASED), b"")?;
            }
        }
        Ok(released)
    }

    /// Gives up every claim `name` holds in the ledger, the boxes it claimed itself as well as those its launch named, because a forgotten session leaves no directory for the exited sweep to find it by.
    pub(super) fn release_all_claims_of(&self, name: &str) -> NorthResult<()> {
        let owners = BTreeSet::from([name.to_owned()]);
        let swept = claims::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
        if swept.unreleased.is_empty() { Ok(()) } else { Err(NorthError::State(format!("{name} is forgotten but the threads ledger did not release all of its claims"))) }
    }

    /// The exited sessions whose claims no sweep has cleared yet, read from their session directories alone.
    pub fn unswept(&self) -> NorthResult<Vec<String>> {
        Ok(self.exited()?.into_iter().filter(|name| !self.root.join(name).join(CLAIMS_RELEASED).exists()).collect())
    }

    /// The file the background claims sweep holds locked while it runs and writes its outcome to.
    pub fn claims_sweep_log(&self) -> PathBuf {
        self.root.join(CLAIMS_SWEEP_LOG)
    }

    /// Reads who holds each of a launch's `items`, and only those, clearing any claim an exited session left on them; an unread ledger is an error the launch reports and leaves to its claim.
    pub fn release_exited_claims_on(&self, items: &[String]) -> NorthResult<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        Ok(claims::release_exited(self.claims.as_ref(), &self.exited()?, Rows::Items(items))?.released)
    }

    fn exited(&self) -> NorthResult<BTreeSet<String>> {
        Ok(self
            .sessions()?
            .iter()
            .filter(|record| !record.verification && !session_running(record) && self.status(record) == Status::Exited)
            .map(|record| record.name.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::fleet::claims::Claims;
    use crate::fleet::fakes::Paper;
    use crate::fleet::model::ProviderName;
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    #[derive(Default)]
    struct Whole(Mutex<Vec<(String, String)>>);

    impl Claims for Whole {
        fn hold(&self, key: &str, session: &str) -> NorthResult<Option<String>> {
            let (issue, boxed) = claims::split_key(key);
            let mut held = self.0.lock().unwrap();
            let beside = held.iter().find(|(claim, owner)| {
                owner.as_str() != session && claims::split_key(claim).1.is_none() && claims::split_key(claim).0 == issue
            });
            if boxed.is_some()
                && let Some((_, owner)) = beside
            {
                return Err(NorthError::Refused(format!("{issue} is already owned by {owner}")));
            }
            held.retain(|(claim, owner)| !(claim.as_str() == key && owner.as_str() == session));
            held.push((key.to_owned(), session.to_owned()));
            Ok(None)
        }

        fn release(&self, key: &str, session: &str) -> NorthResult<()> {
            self.0.lock().unwrap().retain(|(claim, owner)| !(claim.as_str() == key && owner.as_str() == session));
            Ok(())
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(claim, owner)| match rows {
                    Rows::Items(items) => items.iter().any(|item| item.as_str() == claims::split_key(claim).0),
                    Rows::Owners(owners) => owners.contains(owner),
                })
                .cloned()
                .collect())
        }
    }

    fn fleet(home: &Path, claims: Arc<Whole>) -> Fleet {
        Fleet::at_claiming(home.into(), Runtimes::new(Vec::new()), Arc::new(Paper::default()), claims).unwrap()
    }

    fn launch(name: &str, parent: Option<&str>) -> LaunchRequest {
        LaunchRequest {
            verification: false,
            name: name.into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: "/tmp".into(),
            model: None,
            effort: None,
            prompt: String::new(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: parent.map(str::to_owned),
            task_ref: Some("north#902".into()),
            claim_box: Some("the box the lead keeps".into()),
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            slots: Default::default(),
        }
    }

    fn claim_keys(request: &LaunchRequest) -> Vec<String> {
        claims::claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims())
    }

    #[test]
    fn a_lead_holding_an_issue_staffs_a_child_on_its_box_and_refuses_a_launch_outside_its_tree() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Whole::default());
        claims.hold("north#902", "qp90-lead").unwrap();
        let fleet = fleet(home.path(), Arc::clone(&claims));
        fs::create_dir_all(&fleet.root).unwrap();

        let child = launch("n-924-child", Some("qp90-lead"));
        let keys = claim_keys(&child);
        assert_eq!(keys, ["north#902@the box the lead keeps"]);
        fleet.claim_before_start(&child, &keys, false).unwrap();
        assert_eq!(claims.holders(Rows::Owners(&BTreeSet::from(["n-924-child".to_owned()]))).unwrap(), [], "the lead's whole claim covers the box, so the child claims nothing beside it");

        let outsider = launch("n-924-outsider", None);
        let keys = claim_keys(&outsider);
        assert!(matches!(fleet.claim_before_start(&outsider, &keys, false), Err(NorthError::Refused(why)) if why == "north#902 is already owned by qp90-lead"));
    }
}
