use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::limit::stopped_on_limit;
use crate::fleet::records::write_record;
use crate::fleet::remote;
use crate::fleet::model::{Account, Effort, LaunchRequest, Role, SessionRecord, Status};
use crate::fleet::runtime::{Opening, Runtime, new_uuid, now_ms};
use crate::fleet::store::{Claim, SessionRow};

use super::stopping::Halt;
use super::takeover::Predecessor;
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
            return self.launch_into(request, claim, &session_dir);
        }
        fs::create_dir_all(&self.root)?;
        match fs::create_dir(&session_dir) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(NorthError::Usage(format!("a session named {} already exists; --take-over ends it and moves the name and its messages to the new session", request.name)));
            }
            claimed => claimed?,
        }
        self.launch_into(request, claim, &session_dir).inspect_err(|_| {
            let _ = fs::remove_dir(&session_dir);
        })
    }

    fn launch_into(&self, request: &LaunchRequest, claim: Claim, session_dir: &Path) -> NorthResult<SessionRecord> {
        let verification = request.verification || std::env::var_os("NORTH_FLEET_TEST").is_some() || request.parent.as_deref().and_then(|parent| self.record(parent).ok()).is_some_and(|parent| parent.verification);
        if verification && north_base::platform::land_queue::test_root().is_none() {
            return Err(NorthError::Refused("a test launch needs NORTH_TEST_ROOT naming the fixture root its fleet state and land queue stay inside".into()));
        }
        if let Some(parent) = &request.parent {
            self.reaches(parent)?;
        }
        self.refuse_second_proxy(request, claim)?;
        let account = self.account(request)?;
        let runtime = self.runtimes.get(&request.provider)?;
        if let Some(model) = request.model.as_deref().filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; launch one of {}", request.provider.label(), runtime.models().join(", "))));
        }
        let effort = efforts::taken(runtime.as_ref(), request.model.as_deref(), request.effort)?;
        let conversation = match &request.resume {
            Some(kept) => Some(kept.clone()),
            None => runtime.assign_conversation()?,
        };
        self.refuse_on_window(conversation.as_deref())?;
        let tasks = super::claims::claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims());
        let mut record = SessionRecord::launched(request, effort, conversation.clone(), verification, now_ms());
        record.overlay = runtime.overlay(&request.name);
        record.account = account.recorded_label();
        let row = SessionRow { status: Status::Starting.label().to_owned(), ..self.table_row(&record, record.created_ms) };
        self.registrar.register(&row, claim, &request.slots)?;
        self.succeed(&request.name, claim, |predecessor| {
            record.addressed_since_ms = predecessor.as_ref().map(Predecessor::addressed_since);
            self.claim_before_start(request, &tasks, verification)?;
            fs::create_dir_all(session_dir)?;
            let Err(error) = self.bring_up(request, &mut record, &account, runtime.as_ref(), predecessor.as_ref(), conversation.as_deref()) else {
                if let Err(error) = self.inherit_workers(request, &record) {
                    eprintln!("north fleet: {} launched, but its predecessor's workers were not moved to it: {}", record.name, error.user_message());
                }
                return Ok(record);
            };
            Err(match self.discard_failed_launch(session_dir) {
                Ok(()) => error,
                Err(left) => NorthError::State(format!("{}; its directory {} was left: {}", error.user_message(), session_dir.display(), left.user_message())),
            })
        })
    }

    fn bring_up(&self, request: &LaunchRequest, record: &mut SessionRecord, account: &Account, runtime: &dyn Runtime, predecessor: Option<&Predecessor>, conversation: Option<&str>) -> NorthResult<()> {
        let opening = match (&request.resume, predecessor) {
            (Some(kept), Some(predecessor)) => {
                remote::carry_conversation(runtime, request.machine.as_ref(), &self.account(&LaunchRequest { machine: request.machine.clone(), ..predecessor.record().relaunch_request() })?, account, kept)?;
                Opening::Resume(kept)
            }
            (Some(kept), None) => Opening::Resume(kept),
            (None, _) => Opening::Fresh(conversation),
        };
        self.start_claiming(record, account, opening)?;
        if let Some(task_ref) = &record.task_ref {
            self.ledger.join_task(task_ref, &request.name)?;
        }
        self.deliver_first_turn(record, account, &request.prompt)
    }

    /// Ends the host of a failed launch and removes its directory however far the launch got, so a host slow to show its provider's exit leaves nothing under the name; renaming it away first means a watchdog racing on the name cannot leave it half removed.
    fn discard_failed_launch(&self, session_dir: &Path) -> NorthResult<()> {
        Host::new(session_dir.to_path_buf()).terminate()?;
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
        let record = self.reopened(name)?;
        let on_limit = self.recent(self.runtime(&record)?.as_ref(), &record).is_ok_and(|transcript| stopped_on_limit(&transcript, !session_running(&record) && !record.ended()));
        let onto = account.map(|to| self.account(&LaunchRequest { account: to.to_owned(), ..record.relaunch_request() })).transpose()?.filter(|onto| onto.recorded_label() != record.account);
        let Some(onto) = onto else {
            let revived = self.wake_placed(name, system_prompt, placement, effort)?;
            if on_limit {
                self.carry_on(name, "this session was revived after its usage limit ended its turn")?;
            }
            return Ok(revived);
        };
        let conversation = record.conversation.clone().ok_or_else(|| NorthError::State(format!("{name} never started a conversation, so it has none to carry onto {}", onto.label)))?;
        self.refuse_beside_interactive(&record)?;
        let cut_short = self.halt_cut_short(name, self.ordered_by(self.caller().as_deref(), Halt::MovedAccount))?;
        let revived = {
            let _start = self.start_lock(name)?;
            remote::carry_conversation(self.runtime(&record)?.as_ref(), record.machine.as_ref(), &self.account(&record.relaunch_request())?, &onto, &conversation)?;
            self.set_account(name, &onto.recorded_label())?;
            self.revive_held(name, system_prompt, placement, effort).or_else(|error| self.set_account(name, &record.account).and(Err(error)))?
        };
        let _ = self.waits().clear_usage(name);
        if cut_short {
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
        if session_running(&record) {
            return Ok(record);
        }
        self.refuse_beside_interactive(&record)?;
        let conversation = record
            .conversation
            .clone()
            .ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let account = self.account(&record.relaunch_request())?;
        self.start(&mut record, &account, Opening::Resume(&conversation))?;
        Ok(record)
    }
}
