use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::file_prediction;
use crate::fleet::{brief_files, brief_grounding, proposal};
use crate::fleet::host::Host;
use crate::fleet::limit::stopped_on_limit;
use crate::fleet::records::write_record;
use crate::fleet::remote::{self, HooksCheck};
use crate::fleet::model::{Account, Delivery, Effort, LaunchRequest, Predicted, Role, SessionRecord, Status};
use crate::fleet::sender::Sender;
use crate::fleet::runtime::{Opening, new_uuid, now_ms};
use crate::fleet::store::{Claim, SessionRow};

use super::grounding;
use super::reserve;
use super::stopping::Halt;
use super::proxy_window::{held_in_window, hosts_early};
use super::takeover::{Predecessor, Seat};
use super::starting::{OpenedHost, Settling};
use super::{Fleet, efforts, session_running};

impl Fleet {
    pub fn launch(&self, request: &LaunchRequest) -> NorthResult<SessionRecord> {
        self.launch_as(request, Claim::Fresh)
    }

    /// Launches under a name a session may already hold: that session is ended and archived first, and the new one takes every message to the name the old one never took, so two sessions never answer to one name.
    pub fn take_over(&self, request: &LaunchRequest) -> NorthResult<SessionRecord> {
        self.launch_as(request, Claim::TakeOver)
    }

    fn launch_as(&self, request: &LaunchRequest, claim: Claim) -> NorthResult<SessionRecord> {
        let session_dir = self.session_dir(&request.name)?;
        if claim == Claim::TakeOver {
            let _ = self.remove_queued(&request.name);
            if let Some(moved) = self.move_in_place(request) {
                return moved;
            }
            return self.launch_into(request, claim, &session_dir);
        }
        let Some(_launching) = self.try_start_lock(&request.name)? else {
            return Err(NorthError::Usage(format!("a launch of {} is in progress; --take-over waits for it and then replaces its session", request.name)));
        };
        let mut claimed = crate::fleet::session_lifecycle::claim(&session_dir);
        if matches!(&claimed, Err(error) if error.kind() == io::ErrorKind::AlreadyExists) && self.clear_ghost(&request.name)? {
            claimed = crate::fleet::session_lifecycle::claim(&session_dir);
        }
        match claimed {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(NorthError::Usage(format!("a session named {} already exists; --take-over ends it and moves the name and its messages to the new session", request.name)));
            }
            claimed => claimed?,
        }
        let _ = self.remove_queued(&request.name);
        self.launch_into(request, claim, &session_dir).inspect_err(|_| {
            let _ = fs::remove_dir(&session_dir);
        })
    }

    fn launch_into(&self, request: &LaunchRequest, claim: Claim, session_dir: &Path) -> NorthResult<SessionRecord> {
        let _reads = remote::ProfileReads::hold();
        let verification = self.admitted(request, claim)?;
        let account = self.account(request)?;
        let caller = north_base::platform::session_scope::reserve_caller();
        reserve::refusal(&reserve::declared(&self.policy), &account, request.role(), request.parent.as_deref(), caller.as_deref()).map_or(Ok(()), |line| Err(NorthError::Refused(line)))?;
        let runtime = self.runtimes.get(&request.provider)?;
        let hooks = HooksCheck::start(runtime.clone(), &account, request.machine.as_ref());
        if let Some(model) = request.model.as_deref().filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; launch one of {}", request.provider.label(), runtime.models().join(", "))));
        }
        let effort = efforts::taken(runtime.as_ref(), request.model.as_deref(), request.effort, &request.name)?;
        let conversation = match &request.resume {
            Some(kept) => Some(kept.clone()),
            None => runtime.assign_conversation()?,
        };
        self.refuse_on_window(conversation.as_deref())?;
        let tasks = crate::fleet::claims::claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims());
        let spare = (claim == Claim::Fresh).then(|| self.take_spare(request, &account, runtime.as_ref(), effort)).flatten();
        let conversation = spare.as_ref().map_or(conversation, |taken| Some(taken.conversation.clone()));
        let request = &*super::spares::served(request, spare.as_ref());
        let mut record = request.record(effort, conversation.clone(), verification, now_ms());
        record.overlay = runtime.overlay(&request.name);
        record.account = account.recorded_label();
        record.reserve_caller = caller;
        let row = SessionRow { status: Status::Starting.label().to_owned(), ..self.table_row(&record, record.created_ms) };
        let unrecorded = record.clone();
        std::thread::scope(|scope| {
            let shown = scope.spawn(|| first_prompt(request, verification));
            let mut early = hosts_early(claim, &unrecorded).then(|| scope.spawn(|| self.open_host(&unrecorded, &account, fresh_opening(request, conversation.as_deref()), spare.as_ref())));
            let mut joined = || early.take().map(|thread| thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
            let registered = self.register_beside(scope, &row, claim)?;
            let launched = self.succeed(&request.name, claim, |seat| {
                let (tasks, claimed_first) = self.claim_for(seat, request, &mut record, &tasks, verification)?;
                if held_in_window(&record) && seat.held().is_some() && request.resume.is_none() {
                    self.take_over_in_window(seat, &mut record, &account, fresh_opening(request, conversation.as_deref()), &shown.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)).0, hooks)?;
                    return self.settled(&record, Ok(()), registered).map(|()| record);
                }
                let predecessor = seat.hand_to(&mut record)?;
                let brought = std::thread::scope(|beside| {
                    let claiming = beside.spawn(|| if claimed_first { Ok(()) } else { self.claim_before_start(request, &tasks, verification) });
                    let claimed = || claiming.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
                    let (opened, claimed) = self.refused_first(joined(), claimed, session_dir)?;
                    Ok::<_, NorthError>(fs::create_dir_all(session_dir).map_err(|error| NorthError::io_at(error, || format!("creating {}", session_dir.display()))).and_then(|()| self.bring_up(request, &mut record, &account, predecessor.as_ref(), conversation.as_deref(), hooks, (opened, claimed), || shown.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))))
                })?;
                match self.settled(&record, brought, registered) {
                    Ok(()) => Ok(self.launched(request, record)),
                    Err(error) => Err(self.discarded(session_dir, error)),
                }
            });
            if launched.is_err() {
                self.abandon_early(joined(), session_dir);
            }
            launched
        })
    }

    /// Takes over `seat` with a successor held in a window: the window opens before the seat retires, so one that never opens leaves the seat as it was, and the name moves to the successor only once its window is up.
    fn take_over_in_window(&self, seat: &mut Seat<'_>, record: &mut SessionRecord, account: &Account, opening: Opening, prompt: &str, hooks: HooksCheck) -> NorthResult<()> {
        self.open_window(record, account, opening, Some(prompt), hooks)?;
        seat.hand_to(record)?;
        fs::create_dir_all(self.session_dir(&record.name)?)?;
        self.hold_in_window(record)?;
        self.join_launch_task(record)
    }

    fn launched(&self, request: &LaunchRequest, record: SessionRecord) -> SessionRecord {
        if let Err(error) = self.inherit_workers(request, &record) {
            eprintln!("north fleet: {} launched, but its predecessor's workers were not moved to it: {}", record.name, error.user_message());
        }
        super::spares::fill_after(&record);
        record
    }

    fn discarded(&self, session_dir: &Path, error: NorthError) -> NorthError {
        match self.discard_failed_launch(session_dir) {
            Ok(()) => error,
            Err(left) => NorthError::State(format!("{}; its directory {} was left: {}", error.user_message(), session_dir.display(), left.user_message())),
        }
    }

    fn move_in_place(&self, request: &LaunchRequest) -> Option<NorthResult<SessionRecord>> {
        let held = self.record(&request.name).ok()?;
        let same = request.resume.is_some() && held.conversation == request.resume && held.provider == request.provider && held.model == request.model && held.effort == request.effort && !held.adopted();
        if !same {
            return None;
        }
        let moved = self.revive_told(&request.name, None, request.placement.clone(), None, Some(&request.account), Some(&request.prompt));
        Some(moved.and_then(|record| {
            let tasks = crate::fleet::claims::claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims());
            self.claim_before_start(request, &tasks, record.verification).map(|()| record)
        }))
    }

    /// Refuses a launch its test root, parent or role does not allow, and says whether it is a verification launch.
    fn admitted(&self, request: &LaunchRequest, claim: Claim) -> NorthResult<bool> {
        let verification = request.verification || std::env::var_os("NORTH_FLEET_TEST").is_some() || request.parent.as_deref().and_then(|parent| self.record(parent).ok()).is_some_and(|parent| parent.verification);
        if verification && north_base::platform::land_queue::test_root().is_none() {
            return Err(NorthError::Refused("a test launch needs NORTH_TEST_ROOT naming the fixture root its fleet state and land queue stay inside".into()));
        }
        if let Some(parent) = &request.parent {
            self.reaches(parent)?;
        }
        self.refuse_second_proxy(request, claim)?;
        Ok(verification)
    }

    /// Ends a host started beside a registration, start lock or claim that then failed, and removes the directory it ran in.
    pub(super) fn abandon_early(&self, opened: Option<NorthResult<OpenedHost>>, session_dir: &Path) {
        let Some(opened) = opened else { return };
        let ended = match opened {
            Ok(opened) => opened.abandon().and_then(|()| self.discard_failed_launch(session_dir)),
            Err(_) => self.remove_launch_dir(session_dir),
        };
        if let Err(error) = ended {
            eprintln!("north fleet: a launch that did not register left its session host or directory {}: {}", session_dir.display(), error.user_message());
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the launch's parts the host start takes, each borrowed from launch_into")]
    fn bring_up(&self, request: &LaunchRequest, record: &mut SessionRecord, account: &Account, predecessor: Option<&Predecessor>, conversation: Option<&str>, hooks: HooksCheck, (opened, claimed): (Option<NorthResult<OpenedHost>>, impl FnOnce() -> NorthResult<()>), prompt: impl FnOnce() -> (String, Option<Predicted>)) -> NorthResult<()> {
        let opening = match (&request.resume, predecessor) {
            (Some(kept), Some(predecessor)) => {
                remote::carry_conversation(self.runtime(record)?.as_ref(), request.machine.as_ref(), &self.profile(&LaunchRequest { machine: request.machine.clone(), ..LaunchRequest::relaunching(predecessor.record()) })?, account, kept)?;
                Opening::Resume(kept)
            }
            _ => fresh_opening(request, conversation),
        };
        let settling = match opened {
            Some(opened) => self.go_live_claimed(record, opening, hooks, opened?, claimed)?,
            None if held_in_window(record) => {
                claimed()?;
                self.open_proxy_window(record, account, opening, Some(&prompt().0), hooks)?;
                return self.join_launch_task(record);
            }
            None => {
                claimed()?;
                self.start(record, account, opening, hooks)?;
                Settling::default()
            }
        };
        self.join_launch_task(record)?;
        let (prompt, predicted) = prompt();
        if predicted.is_some() {
            record.predicted = predicted;
            write_record(&self.session_dir(&request.name)?, record)?;
        }
        self.deliver_first_turn(record, account, &prompt, settling)
    }

    fn join_launch_task(&self, record: &SessionRecord) -> NorthResult<()> {
        record.task_ref.as_deref().map_or(Ok(()), |task_ref| self.ledger.join_task(task_ref, &record.name))
    }

    /// Ends the host of a failed launch and removes its directory however far the launch got, so a host slow to show its provider's exit leaves nothing under the name; renaming it away first means a watchdog racing on the name cannot leave it half removed.
    pub(super) fn discard_failed_launch(&self, session_dir: &Path) -> NorthResult<()> {
        Host::new(session_dir.to_path_buf()).terminate()?;
        self.remove_launch_dir(session_dir)
    }

    fn remove_launch_dir(&self, session_dir: &Path) -> NorthResult<()> {
        let discarded = self.root.join(format!(".failed-launch-{}", new_uuid()?));
        fs::rename(session_dir, &discarded)?;
        fs::remove_dir_all(&discarded)?;
        Ok(())
    }

    fn refuse_second_proxy(&self, request: &LaunchRequest, claim: Claim) -> NorthResult<()> {
        if request.role() != Role::Proxy {
            return Ok(());
        }
        let live = self.sessions()?.into_iter().filter(|record| record.role() == Role::Proxy && self.status(record) != Status::Exited);
        match live.into_iter().find(|record| claim == Claim::Fresh || record.name != request.name) {
            Some(proxy) => Err(NorthError::Usage(format!("{} is the live proxy; a proxy launch with --take-over ends it and moves the role to the new session", proxy.name))),
            None => Ok(()),
        }
    }

    pub fn revive(&self, name: &str, system_prompt: Option<PathBuf>) -> NorthResult<SessionRecord> {
        self.revive_placed(name, system_prompt, None, None, None)
    }

    /// Revive with a fresh placement override note and effort, which the restarted record keeps. With `account` naming another account, a running host ends first and its conversation is carried there and resumes on it; a restart that fails leaves the session stopped on its old account.
    pub fn revive_placed(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>, account: Option<&str>) -> NorthResult<SessionRecord> {
        self.revive_told(name, system_prompt, placement, effort, account, None)
    }

    fn revive_told(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>, account: Option<&str>, note: Option<&str>) -> NorthResult<SessionRecord> {
        let record = self.reopened(name)?;
        let on_limit = self.recent(self.runtime(&record)?.as_ref(), &record).is_ok_and(|transcript| stopped_on_limit(&transcript, !session_running(&record) && !record.ended()));
        let onto = account.map(|to| self.account(&LaunchRequest { account: to.to_owned(), ..LaunchRequest::relaunching(&record) })).transpose()?.filter(|onto| onto.recorded_label() != record.account);
        let Some(onto) = onto else {
            let reserve = reserve::declared(&self.policy);
            if reserve::on_reserve(&reserve, &record) {
                return Err(NorthError::Refused(format!("{name} is recorded on {}, which it may not continue on: {}; revive it with --account naming a plan account", record.account, reserve.why(&record.account))));
            }
            let revived = self.wake_placed(name, system_prompt, placement, effort)?;
            if let Some(note) = note {
                self.send_signed(&Sender::fleet(), name, note, Delivery::Queued)?;
            } else if on_limit {
                self.carry_on(name, "this session was revived after its usage limit ended its turn")?;
            }
            return Ok(revived);
        };
        self.refuse_reserve(&record, &onto)?;
        let conversation = record.conversation.clone().ok_or_else(|| NorthError::State(format!("{name} never started a conversation, so it has none to carry onto {}", onto.label)))?;
        self.refuse_beside_interactive(&record)?;
        let cut_short = self.halt_cut_short(name, self.ordered_by(self.caller().as_deref(), Halt::MovedAccount))?;
        let revived = {
            let _start = self.start_lock(name)?;
            remote::carry_conversation(self.runtime(&record)?.as_ref(), record.machine.as_ref(), &self.profile(&LaunchRequest::relaunching(&record))?, &onto, &conversation)?;
            self.set_account(name, &onto.recorded_label())?;
            self.revive_held(name, system_prompt, placement, effort).or_else(|error| self.set_account(name, &record.account).and(Err(error)))?
        };
        let _ = self.waits().clear_usage(name);
        if let Some(note) = note {
            self.send_signed(&Sender::fleet(), name, note, Delivery::Queued)?;
        } else if cut_short {
            self.carry_on(name, &format!("this session was moved onto account {} in the middle of a turn", onto.label))?;
        } else if on_limit {
            self.carry_on(name, &format!("this session was moved onto account {} after its usage limit ended its turn", onto.label))?;
        }
        Ok(revived)
    }

    /// Revives `name` for a message the caller hands it next, so it sends no carry-on of its own beside that message.
    pub(super) fn wake(&self, name: &str) -> NorthResult<SessionRecord> {
        self.wake_placed(name, None, None, None)
    }

    fn wake_placed(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        let revived = {
            let _start = self.start_lock(name)?;
            self.revive_held(name, system_prompt, placement, effort)?
        };
        let _ = self.waits().clear_usage(name);
        Ok(revived)
    }

    fn set_account(&self, name: &str, account: &str) -> NorthResult<()> {
        let mut record = self.record(name)?;
        account.clone_into(&mut record.account);
        write_record(&self.session_dir(name)?, &record)
    }

    pub(super) fn revive_held(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        let mut record = self.record(name)?;
        if record.adopted() {
            return Err(NorthError::Refused(format!("{name} is held by an interactive window north fleet adopt bound to it, so the fleet starts no host under the name")));
        }
        record.effort = efforts::revived(&record, self.runtime(&record)?.as_ref(), session_running(&record), effort)?;
        if system_prompt.is_some() {
            record.system_prompt = system_prompt;
        }
        if placement.is_some() {
            record.placement = placement;
        }
        let host = Host::new(record.host_dir.clone());
        if session_running(&record) && !host.closed() {
            return Ok(record);
        }
        self.refuse_beside_interactive(&record)?;
        if session_running(&record) {
            host.terminate()?;
        }
        let conversation = record
            .conversation
            .clone()
            .ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let account = self.account(&LaunchRequest::relaunching(&record))?;
        let hooks = HooksCheck::start(self.runtime(&record)?, &account, record.machine.as_ref());
        self.start(&mut record, &account, Opening::Resume(&conversation), hooks)?;
        Ok(record)
    }
}

fn first_prompt(request: &LaunchRequest, verification: bool) -> (String, Option<Predicted>) {
    if request.role() != Role::Worker || request.resume.is_some() {
        return (request.prompt.clone(), None);
    }
    let brief = request.named_brief().map_or_else(|| request.prompt.clone(), |named| format!("{named}\n{}", request.prompt));
    let arm = grounding::drawn(&request.name, now_ms());
    let prompt = match arm {
        Some(arm) if !verification => grounding::stamped(&request.prompt, arm),
        _ => request.prompt.clone(),
    };
    std::thread::scope(|scope| {
        let verdict = scope.spawn(|| launch_verdict(request, verification));
        let predicted = scope.spawn(|| if verification { None } else { file_prediction::section(&prompt, &request.cwd, request.machine.as_ref(), &north_base::platform::north_binary::own_binary()) });
        let grounded = scope.spawn(|| if verification || !grounding::carries_pack(arm) { None } else { brief_grounding::section(&brief, &request.cwd, request.machine.as_ref(), &north_base::platform::north_binary::own_binary()) });
        let shown = brief_files::shown(&prompt, &request.cwd, request.machine.as_ref());
        let (guess, record) = predicted.join().unwrap_or(None).map_or((None, None), |(section, predicted)| (section, Some(predicted)));
        let shown = guess.map_or(shown.clone(), |section| format!("{shown}\n\n{section}"));
        let shown = grounded.join().unwrap_or(None).map_or(shown.clone(), |section| format!("{shown}\n\n{section}"));
        (proposal::told(&shown, &verdict.join().unwrap_or(proposal::Verdict::Unasked)), record)
    })
}

fn launch_verdict(request: &LaunchRequest, verification: bool) -> proposal::Verdict {
    let one_call = north_base::platform::harness::document("harness/worker-tools.toml")["verbs"].as_array().is_some_and(|verbs| verbs.iter().any(|verb| verb.as_str() == Some("finish")));
    let message = proposal::message(&request.prompt).filter(|_| !verification && one_call && proposal::in_band(&request.prompt)).unwrap_or_default();
    let north = match request.machine {
        Some(_) => north_base::platform::north_binary::north_binary(),
        None => north_base::platform::north_binary::own_binary(),
    };
    proposal::judge((!verification).then_some(request.prompt.as_str()), &message, &request.name, &request.cwd, &north, request.machine.as_ref())
}

fn fresh_opening<'a>(request: &'a LaunchRequest, conversation: Option<&'a str>) -> Opening<'a> {
    match &request.resume {
        Some(kept) => Opening::Resume(kept),
        None => Opening::Fresh(conversation),
    }
}
