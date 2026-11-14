use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::claim_releases;
use crate::fleet::claims::{self, Rows, Taken};
use crate::fleet::model::{LaunchRequest, SessionRecord, Status};
use crate::fleet::records::{CLAIMS_RELEASED, CLAIMS_SWEEP_LOG, read_record, write_record};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::LAUNCH_GRACE_MS;

use super::spooled::Holding;
use super::starting::OpenedHost;
use super::{Fleet, session_running};

/// How many owners one `threads holder` read names, so a fleet with hundreds of exited sessions never asks for them all in a read the ledger cannot answer within its bound.
const OWNERS_PER_READ: usize = 64;

/// Whether a session's record names no host and no window long after its launch: the host never started, so the session holds nothing.
pub(super) fn never_started(record: &SessionRecord) -> bool {
    !record.adopted() && record.host_pid.is_none() && record.host_dir.as_os_str().is_empty() && now_ms().saturating_sub(record.created_ms) > LAUNCH_GRACE_MS
}

/// A host a launch opened beside its claims, if it opened one there.
type Early = Option<NorthResult<OpenedHost>>;

/// The wait on a launch's claims that its first turn takes.
type ClaimGate<'a> = Box<dyn FnOnce() -> NorthResult<()> + 'a>;

impl Fleet {
    /// Claims every task of a launch before its session starts, first clearing any claim an exited session left on them: a live holder refuses it. A `--test` session claims nothing. It runs beside the host start and, for a fresh launch, beside its registration with the ledger.
    pub(super) fn claim_before_start(&self, request: &LaunchRequest, tasks: &[String], verification: bool) -> NorthResult<()> {
        let items = request.tasks();
        if let Err(error) = self.release_exited_claims_on(&items) {
            eprintln!("north fleet: {} launches with its holder check left to its claim, because the ledger did not say who holds {}: {}", request.name, items.join(" "), error.user_message());
        }
        if verification {
            return Ok(());
        }
        self.claim_tasks(request.parent.as_deref(), &request.name, tasks)
    }

    /// The host a launch opened beside its claims and the claims still to wait on, unless the host failed to start and the claims were refused: the refusal is then the launch's answer, and the directory the host left is removed.
    pub(super) fn refused_first<'a>(&self, opened: Early, claimed: impl FnOnce() -> NorthResult<()> + 'a, session_dir: &Path) -> NorthResult<(Early, ClaimGate<'a>)> {
        let Some(Err(failed)) = opened else { return Ok((opened, Box::new(claimed))) };
        if let Err(refused) = claimed() {
            self.abandon_early(Some(Err(failed)), session_dir);
            return Err(refused);
        }
        Ok((Some(Err(failed)), Box::new(|| Ok(()))))
    }

    /// The claim keys of a taken-over seat its successor `name` still has to take: a key the name or a session below it already holds stays with its holder, so the seat's children keep the issues they work and a refused take-over gives up none of the old seat's claims. A proxy that `routes` its tasks rather than working them also leaves an issue any other session holds to that session.
    pub(super) fn left_to_claim(&self, name: &str, keys: &[String], routes: bool) -> Vec<String> {
        let issues: Vec<String> = keys.iter().map(|key| claims::split_key(key).0.to_owned()).collect();
        let Ok(rows) = self.claims.holders(Rows::Items(&issues)) else { return keys.to_vec() };
        let below = self.descendants(name);
        let kept = |key: &String| rows.iter().any(|(held, owner)| {
            let ours = (owner == name || below.contains(owner)) && (self.claims.covers(held, key) || (claims::split_key(key).1.is_none() && claims::split_key(held).0 == key.as_str()));
            ours || (routes && owner != name && claims::split_key(held).0 == claims::split_key(key).0)
        });
        keys.iter().filter(|key| !kept(key)).cloned().collect()
    }

    /// Claims `tasks` for `name` under `parent`, leaving the whole-issue claims its ancestors hold and taking the boxes.
    fn claim_tasks(&self, parent: Option<&str>, name: &str, tasks: &[String]) -> NorthResult<()> {
        let ancestors = self.ancestors(parent);
        claims::hold_all(self.claims.as_ref(), &claims::own_tasks(self.claims.as_ref(), tasks, &ancestors), name, &ancestors)
    }
}

impl Fleet {
    /// Adds `task` to the issues the running session `name` holds, as a launch's `--task` would have: the session's record names it, so `north fleet list --json` shows it to every dispatch whatever the threads ledger says, and its claim in the ledger is taken on top. A ledger that cannot be read is a warning, never a failure, because the record already holds the issue.
    pub fn hold_task(&self, name: &str, task: &str) -> NorthResult<()> {
        if self.writes_records() {
            return self.hold_task_here(name, task);
        }
        self.spool_hold(name, task, Holding::Take).map(|row| println!("spooled HOLD row {}: {name} holds {task} once this host's fleet tick applies it", row.id))
    }

    pub(super) fn hold_task_here(&self, name: &str, task: &str) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        if record.verification {
            return Ok(());
        }
        if record.hold(task) {
            write_record(&session_dir, &record)?;
        }
        match self.claims.hold(task, name, &self.ancestors(record.parent.as_deref())) {
            Ok(Taken::Moved(moved)) => println!("{moved}"),
            Ok(Taken::Queued(_)) => println!("{name} holds {task}; its claim is queued and lands once the ledger answers"),
            Ok(Taken::Held) => println!("{name} holds {task}"),
            Err(error) => eprintln!("north fleet: {name} holds {task} in its record, but the threads ledger did not take its claim: {}", error.user_message()),
        }
        Ok(())
    }

    /// Undoes [`Self::hold_task`]: the session's record drops `task`, so `north fleet list --json` stops showing it to dispatch, and every claim the session holds on its issue is given up. A ledger that cannot be read is a warning, because the record no longer holds the issue.
    pub fn release_task(&self, name: &str, task: &str) -> NorthResult<()> {
        if self.writes_records() {
            return self.release_task_here(name, task);
        }
        self.spool_hold(name, task, Holding::Drop).map(|row| println!("spooled HOLD row {}: {name} stops holding {task} once this host's fleet tick applies it", row.id))
    }

    pub(super) fn release_task_here(&self, name: &str, task: &str) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        let dropped = record.release(task);
        if dropped {
            write_record(&session_dir, &record)?;
        }
        match claims::release_task(self.claims.as_ref(), name, task) {
            Ok(released) if dropped || released > 0 => println!("{name} no longer holds {task}"),
            Ok(_) => println!("{name} did not hold {task}"),
            Err(error) => eprintln!("north fleet: {name} no longer holds {task} in its record, but the threads ledger did not release its claim: {}", error.user_message()),
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
            let swept = claim_releases::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
            released += swept.released;
            let exited = self.exited()?;
            for owner in owners.difference(&swept.unreleased).filter(|owner| exited.contains(*owner)) {
                fs::write(self.root.join(owner).join(CLAIMS_RELEASED), b"")?;
            }
        }
        Ok(released)
    }

    /// Gives up every claim `name` holds in the ledger, the boxes it claimed itself as well as those its launch named, because a forgotten or retired session leaves no directory for the exited sweep to find it by.
    pub(super) fn release_all_claims_of(&self, name: &str) -> NorthResult<()> {
        let owners = BTreeSet::from([name.to_owned()]);
        let swept = claim_releases::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
        if swept.unreleased.is_empty() { Ok(()) } else { Err(NorthError::State(format!("{name} ends but the threads ledger did not release all of its claims"))) }
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
        let mut gone = self.exited()?;
        let now = now_ms();
        gone.extend(self.cached_sessions()?.unwrap_or_default().into_iter().filter(|row| row.never_started(now)).map(|row| row.name));
        Ok(claim_releases::release_exited(self.claims.as_ref(), &gone, Rows::Items(items))?.released)
    }

    /// The sessions that hold nothing: those that exited, and those whose host never started within the launch grace.
    fn exited(&self) -> NorthResult<BTreeSet<String>> {
        Ok(self
            .sessions()?
            .iter()
            .filter(|record| !record.verification && (never_started(record) || (!session_running(record) && self.status(record) == Status::Exited)))
            .map(|record| record.name.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::fleet::claims::Claims;
    use crate::fleet::fakes::Paper;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    /// A ledger with the claim rule: a live whole-issue claim refuses a box claim beside it unless its owner is above
    /// the claimant.
    #[derive(Default)]
    struct Whole(Mutex<Vec<(String, String)>>);

    impl Claims for Whole {
        fn hold(&self, item: &str, session: &str, ancestors: &BTreeSet<String>) -> NorthResult<Taken> {
            let (issue, boxed) = claims::split_key(item);
            let mut held = self.0.lock().unwrap();
            let outside = held.iter().find(|(claim, owner)| {
                owner.as_str() != session && claims::split_key(claim).1.is_none() && claims::split_key(claim).0 == issue && !ancestors.contains(owner)
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
                    Rows::Items(items) => items.iter().any(|item| item == claims::split_key(claim).0),
                    Rows::Owners(owners) => owners.contains(owner),
                })
                .cloned()
                .collect())
        }
    }

    #[test]
    fn a_lead_holding_an_issue_staffs_a_child_on_its_box_and_refuses_a_launch_outside_its_tree() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Whole::default());
        claims.hold("north#902", "qp90-lead", &BTreeSet::new()).unwrap();
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(Vec::new()), Arc::new(Paper::default()), claims.clone()).unwrap();
        fs::create_dir_all(&fleet.root).unwrap();
        let boxed = ["north#902@the box the lead keeps".to_owned()];

        fleet.claim_tasks(Some("qp90-lead"), "n-924-child", &boxed).unwrap();
        assert_eq!(claims.holders(Rows::Owners(&BTreeSet::from(["n-924-child".to_owned()]))).unwrap(), [(boxed[0].clone(), "n-924-child".to_owned())]);

        let refused = fleet.claim_tasks(None, "n-924-outsider", &boxed).unwrap_err();
        assert!(matches!(refused, NorthError::Refused(why) if why == "north#902 is already owned by qp90-lead"));
    }
}

#[cfg(test)]
mod launch_tests {
    use std::sync::Arc;

    use crate::fleet::claims::{Claims, Held};
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::model::{LaunchRequest, ProviderName};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    fn request(cwd: &std::path::Path) -> LaunchRequest {
        LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name: "north-1042-d0525".into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: cwd.into(),
            model: None,
            effort: None,
            prompt: "Item: north#1042".into(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: None,
            task_ref: Some("north#1042".into()),
            claim_box: None,
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            dispatched: false,
            surface: Default::default(),
            route: None,
                    }
    }

    /// On 2026-11-10 dispatch launches of north#1042 failed at start and the claim stayed with the dead launch.
    #[test]
    fn a_launch_whose_host_fails_at_start_leaves_no_claim_behind() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap();

        let launched = fleet.launch(&request(&home.path().join("no-such-directory")));

        let error = launched.unwrap_err().user_message();
        let tasks = ["north#1042".to_owned()];
        assert_eq!(claims.holders(Rows::Items(&tasks)).unwrap(), Vec::<(String, String)>::new());
        assert!(claims.0.lock().unwrap().iter().any(|(kind, _, _)| *kind == "hold"), "the launch took its claim before the host failed: {error}");
    }

    struct HeldElsewhere;

    impl crate::fleet::store::Registrar for HeldElsewhere {
        fn register(&self, row: &crate::fleet::store::SessionRow, _: crate::fleet::store::Claim) -> NorthResult<()> {
            Err(NorthError::Refused(format!("the session name {} is held fleet-wide by a working session on cpx62b", row.name)))
        }

        fn retire_unplaced(&self) -> NorthResult<Vec<String>> {
            Ok(Vec::new())
        }
    }

    struct Target(std::path::PathBuf);

    impl crate::fleet::registry::HeldUnits for Target {
        fn retire(&self, held: &crate::fleet::remote::HeldOn) -> NorthResult<()> {
            let unit = self.0.join(&held.unit);
            let pid = std::fs::read_to_string(unit.join("pid"))?;
            std::process::Command::new("kill").args(["-KILL", pid.trim()]).status()?;
            std::fs::remove_dir_all(unit)?;
            Ok(())
        }
    }

    fn sleeping(pids: &std::path::Path) -> u32 {
        use std::io::Write;
        use std::os::unix::process::CommandExt;
        let pid = std::process::Command::new("sleep").arg("300").process_group(0).spawn().unwrap().id();
        std::fs::OpenOptions::new().create(true).append(true).open(pids).unwrap().write_all(format!("{pid}\n").as_bytes()).unwrap();
        pid
    }

    fn target_host(dir: &std::path::Path, spec: &crate::fleet::runtime::HostSpec, provider: &ProviderName, idle: Option<std::time::Duration>, served_by: &str) -> NorthResult<u32> {
        let root = dir.parent().unwrap();
        let unit = root.join("target").join(north_session_host::held::unit(&spec.key));
        std::fs::create_dir_all(&unit)?;
        std::fs::write(unit.join("pid"), sleeping(&root.join("pids")).to_string())?;
        super::super::fixtures::standing_host(dir, spec, provider, idle, served_by)?;
        let link = sleeping(&root.join("pids"));
        std::fs::write(north_base::platform::session_host_files::pid_path(dir), link.to_string())?;
        Ok(link)
    }

    fn alive(pid: &str) -> bool {
        std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output().is_ok_and(|ps| ps.status.success() && !String::from_utf8_lossy(&ps.stdout).trim_start().starts_with('Z'))
    }

    #[test]
    fn a_launch_refused_after_placement_leaves_no_unit_held_directory_or_process_on_the_target_host() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap().registering(Arc::new(HeldElsewhere)).arming(|_| Ok(())).starting_hosts(target_host);
        let target = fleet.root.join("target");
        let fleet = fleet.retiring_held(Arc::new(Target(target.clone())));
        claims.hold("north#1042", "north-1042-d0525", &BTreeSet::new()).unwrap();
        let placed = LaunchRequest { machine: Some(crate::fleet::model::Machine { name: "cpx62".into(), ssh: "tom@cpx62.invalid".into(), agent_ssh: None }), ..request(home.path()) };

        let launched = fleet.launch(&placed);

        let error = launched.unwrap_err().user_message();
        assert!(claims.0.lock().unwrap().iter().all(|(kind, _, _)| *kind != "release"), "the refused launch released the claims of the session holding its name: {error}");
        let left: Vec<_> = std::fs::read_dir(&target).unwrap_or_else(|_| panic!("the launch never placed its host: {error}")).flatten().map(|entry| entry.file_name()).collect();
        assert!(left.is_empty(), "the refused launch left held units {left:?} on its target: {error}");
        let pids = std::fs::read_to_string(fleet.root.join("pids")).unwrap();
        let running: Vec<_> = pids.lines().filter(|pid| alive(pid)).collect();
        assert!(running.is_empty(), "the refused launch left processes {running:?} running: {error}");
    }
}

#[cfg(test)]
mod ghost_tests {
    use std::sync::Arc;

    use crate::fleet::claims::{Claims, Held};
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::launch_queue::{Priority, QueuedLaunch};
    use crate::fleet::model::{LaunchRequest, ProviderName};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    fn request(cwd: &std::path::Path) -> LaunchRequest {
        LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name: "safety-lead".into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: cwd.into(),
            model: None,
            effort: None,
            prompt: "Item: north#1054".into(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: None,
            task_ref: Some("north#1054".into()),
            claim_box: None,
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            dispatched: false,
            surface: Default::default(),
            route: None,
        }
    }

    /// On 2026-11-11 a launch of safety-lead killed during its claims and a second one queued left a name that `launch` called taken while `show` and `--take-over` found no session.
    #[test]
    fn an_interrupted_or_queued_launch_leaves_a_name_a_later_launch_or_take_over_can_use() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap();
        let wanted = request(&home.path().join("no-such-directory"));
        let tasks = ["north#1054".to_owned()];

        for take_over in [false, true] {
            let _ = fs::remove_dir_all(fleet.root.join("safety-lead"));
            fs::create_dir_all(fleet.root.join("safety-lead")).unwrap();
            claims.hold("north#1054", "safety-lead", &BTreeSet::new()).unwrap();
            fleet.queue_launch(&QueuedLaunch::new(wanted.clone(), Priority::Normal, 1, "provider limit".into())).unwrap();
            assert_eq!(fleet.queued_launches().unwrap().len(), 1);

            let launched = if take_over { fleet.take_over(&wanted) } else { fleet.launch(&wanted) };

            let error = launched.unwrap_err().user_message();
            assert!(!error.contains("already exists") && !error.contains("no session named"), "take_over={take_over}: {error}");
            let held: Vec<_> = claims.holders(Rows::Items(&tasks)).unwrap().into_iter().filter(|(_, owner)| owner == "safety-lead").collect();
            assert_eq!(held, Vec::<(String, String)>::new(), "take_over={take_over}");
            assert_eq!(fleet.queued_launches().unwrap().len(), 0, "take_over={take_over}: the queued launch outlived the launch that replaced it");
        }
    }
}

#[cfg(test)]
mod queued_claim_tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::fleet::claims::Claims;
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::model::{LaunchRequest, ProviderName};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    /// A ledger host too slow to answer a claim in time: the claim waits in a spool and lands, ahead of any later one, when the ledger is next read.
    #[derive(Default)]
    struct Spooled {
        waiting: Mutex<Vec<(String, String)>>,
        landed: Mutex<Vec<(String, String)>>,
    }

    impl Claims for Spooled {
        fn hold(&self, item: &str, session: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            self.waiting.lock().unwrap().push((item.to_owned(), session.to_owned()));
            Ok(Taken::Queued("spooled".to_owned()))
        }

        fn queued_claim_wait(&self) -> Duration {
            Duration::ZERO
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.landed.lock().unwrap().retain(|(claim, owner)| !(claim == item && owner == session));
            Ok(())
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            let mut landed = self.landed.lock().unwrap();
            for (item, session) in self.waiting.lock().unwrap().drain(..) {
                if !landed.iter().any(|(claim, _)| *claim == item) {
                    landed.push((item, session));
                }
            }
            Ok(landed.clone())
        }
    }

    fn request(name: &str, cwd: &std::path::Path) -> LaunchRequest {
        LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name: name.into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: cwd.into(),
            model: None,
            effort: None,
            prompt: "Item: north#1106".into(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: None,
            task_ref: Some("north#1106".into()),
            claim_box: None,
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            dispatched: false,
            surface: Default::default(),
            route: None,
        }
    }

    /// On 2026-11-11 dispatch staffed north#1106's replay box twice on one host: the first launch went ahead on a claim the ledger had only queued, so the second saw no holder.
    #[test]
    fn a_launch_whose_claim_is_only_queued_does_not_start_a_second_worker_on_the_box() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Spooled::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), ledger.clone()).unwrap();
        let tasks = ["north#1106".to_owned()];
        ledger.hold("north#1106", "north-1106-first", &BTreeSet::new()).unwrap();

        let second = fleet.launch(&request("north-1106-second", &home.path().join("no-such-directory"))).unwrap_err();

        assert!(matches!(&second, NorthError::Refused(why) if why == "north#1106 is already owned by north-1106-first"), "{}", second.user_message());
        assert_eq!(ledger.holders(Rows::Items(&tasks)).unwrap(), [("north#1106".to_owned(), "north-1106-first".to_owned())]);
        assert!(!fleet.root.join("north-1106-second").exists(), "the refused launch left a session directory");
    }
}

#[cfg(test)]
mod never_started_tests {
    use std::sync::Arc;

    use crate::fleet::claims::{Claims, Held};
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::runtimes::Runtimes;

    use super::*;

    /// On 2026-11-10 north-1042-d0525 never started and `sweep-claims` released 0, because the session never counted as exited.
    #[test]
    fn the_sweep_releases_the_claims_of_a_session_whose_host_never_started() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap();
        let dir = fleet.root.join("north-1042-d0525");
        fs::create_dir_all(&dir).unwrap();
        let record = serde_json::json!({"name": "north-1042-d0525", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": null, "host_dir": "", "stream_offset": 0});
        fs::write(dir.join("session.json"), record.to_string()).unwrap();
        claims.hold("north#1042", "north-1042-d0525", &BTreeSet::new()).unwrap();
        let tasks = ["north#1042".to_owned()];

        assert_eq!(fleet.unswept().unwrap(), ["north-1042-d0525"]);
        assert_eq!(fleet.release_exited_claims().unwrap(), 1);

        assert_eq!(claims.holders(Rows::Items(&tasks)).unwrap(), Vec::<(String, String)>::new());
        assert!(fleet.live_owners().unwrap().is_empty());
    }
}
