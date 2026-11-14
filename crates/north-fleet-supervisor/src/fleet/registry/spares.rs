use std::fs::OpenOptions;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, Effort, LaunchRequest, Machine, Role, SessionRecord};
use crate::fleet::runtime::{HostSpec, Opening, Runtime, binary, new_uuid};
use crate::fleet::spares::{self, Place, Taken};

use super::{Fleet, efforts, reserve};

impl Fleet {
    /// A spare on the request's build host whose spec is the one this launch would start, taken and bound to the launch's name; None for any launch a spare cannot serve, and when none fits.
    pub(super) fn take_spare(&self, request: &LaunchRequest, account: &Account, runtime: &dyn Runtime, effort: Option<Effort>) -> Option<Taken> {
        let machine = request.machine.as_ref().filter(|_| request.role() == Role::Worker && request.resume.is_none())?;
        let fingerprint = spares::fingerprint(&self.worker_spec(request, account, runtime, effort, &new_uuid().ok()?).ok()?, spares::current_runtime().as_deref());
        if !std::fs::read_to_string(self.spares_hint(machine, &fingerprint)).is_ok_and(|count| count.trim() != "0") {
            return None;
        }
        let (ssh, home) = (binary("ssh"), home()?);
        spares::take(&Place { ssh: &ssh, machine, home: &home }, &fingerprint, &request.name, &request.cwd, &spares::binding(&request.name, request.capacity_need.as_deref()))
    }

    pub(super) fn release_spare(&self, machine: &Machine, taken: &Taken) {
        let (ssh, Some(home)) = (binary("ssh"), home()) else { return };
        spares::release(&Place { ssh: &ssh, machine, home: &home }, taken);
    }

    /// `north spare-fill NAME`: keeps spares for launches like the worker `name` on its build host.
    pub fn fill_spares(&self, name: &str) -> NorthResult<String> {
        let record = self.record(name)?;
        self.fill_for(&LaunchRequest::relaunching(&record), record.effort)
    }

    /// `north spare-fill --host HOST -- LAUNCH-WORDS`: keeps spares for the launch `request` before the first launch like it, at the effort that launch would take.
    pub fn fill_spares_ahead(&self, request: &LaunchRequest) -> NorthResult<String> {
        let runtime = self.runtimes.get(&request.provider)?;
        self.fill_for(request, efforts::taken(runtime.as_ref(), request.model.as_deref(), request.effort, &request.name)?)
    }

    /// Keeps spares for launches like the worker `request` at `effort` on its build host, retiring those whose policy text, settings or runtime are no longer current.
    fn fill_for(&self, request: &LaunchRequest, effort: Option<Effort>) -> NorthResult<String> {
        let machine = request.machine.clone().filter(|_| request.role() == Role::Worker).ok_or_else(|| NorthError::Usage(format!("{} is not a worker on a build host, so no spare serves launches like it", request.name)))?;
        let runtime = self.runtimes.get(&request.provider)?;
        let template = spares::template(request.clone(), spares::spare_name()?);
        let account = self.account(&template)?;
        if let Some(line) = reserve::refusal(&reserve::declared(&self.policy), &account, Role::Worker, template.parent.as_deref(), None) {
            return Err(NorthError::Refused(line));
        }
        let (ssh, home) = (binary("ssh"), home().ok_or_else(|| NorthError::State("HOME is not set".into()))?);
        let place = Place { ssh: &ssh, machine: &machine, home: &home };
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(self.root.join(format!(".spares-{}.lock", machine.name.replace('/', "_"))))?;
        if lock.try_lock().is_err() {
            return Ok(format!("a fill for {} is already running", machine.name));
        }
        let pool = spares::survey(&place).ok_or_else(|| NorthError::State(format!("{} did not answer the spare survey", machine.name)))?;
        let kind = spares::kind(&account.recorded_label(), template.model.as_deref(), effort.map(Effort::label), template.window(), &template.cwd);
        let fingerprint = spares::fingerprint(&self.worker_spec(&template, &account, runtime.as_ref(), effort, &new_uuid()?)?, spares::current_runtime().as_deref());
        let (stale, start) = spares::wanted(&pool, &kind, &fingerprint);
        spares::retire(&place, &stale);
        for retired in &stale {
            let _ = std::fs::remove_file(self.spares_hint(&machine, retired.split('-').nth(1).unwrap_or_default()));
        }
        let held = pool.entries.iter().filter(|entry| entry.starts_with(&format!("{kind}-{fingerprint}-"))).count();
        for _ in 0..start {
            let (spare, conversation) = (spares::spare_name()?, new_uuid()?);
            let cwd = spares::lane_root(&template.cwd).map_or_else(|| template.cwd.clone(), |root| spares::spare_lane(&root, &spare));
            let request = LaunchRequest { name: spare.clone(), cwd, ..template.clone() };
            let spec = spares::spare_spec(self.worker_spec(&request, &account, runtime.as_ref(), effort, &conversation)?, &place, &spare);
            spares::start(&place, &spec, &kind, &fingerprint, &spare, &conversation)?;
        }
        std::fs::write(self.spares_hint(&machine, &fingerprint), (held + start).to_string())?;
        Ok(format!("retired {} and started {start} spares on {}", stale.len(), machine.name))
    }

    /// The file where the last fill on `machine` left how many spares of `fingerprint` it holds, so a launch asks the host for one only when there may be one.
    fn spares_hint(&self, machine: &Machine, fingerprint: &str) -> PathBuf {
        self.root.join(format!(".spares-{}-{fingerprint}", machine.name.replace('/', "_")))
    }

    fn worker_spec(&self, request: &LaunchRequest, account: &Account, runtime: &dyn Runtime, effort: Option<Effort>, conversation: &str) -> NorthResult<HostSpec> {
        let record = request.record(effort, Some(conversation.to_owned()), false, 0);
        let (instructions, _) = self.instructions(&record)?;
        runtime.host(account, &LaunchRequest::relaunching(&record), Opening::Fresh(Some(conversation)), &instructions)
    }
}

/// Starts `north spare-fill NAME` apart from this launch, so the next launch like it finds a spare.
pub(super) fn fill_after(record: &SessionRecord) {
    if !fill_wanted(record) {
        return;
    }
    let Ok(program) = std::env::current_exe() else { return };
    if program.file_name().is_none_or(|name| name != "north") {
        return;
    }
    let _ = std::os::unix::process::CommandExt::process_group(std::process::Command::new(program).args(["spare-fill", &record.name]), 0).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

/// Whether a launch like `record` keeps spares: a worker on a build host, never a test launch nor a canary the runtime promotion started.
fn fill_wanted(record: &SessionRecord) -> bool {
    record.machine.is_some() && record.role() == Role::Worker && !record.verification && record.reserve_caller.as_deref() != Some(north_base::platform::session_scope::RESERVE_CALLER_UNIT)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from)
}

/// `request` as the spare it took serves it: in the spare's directory when the take moved it into the spare's lane.
pub(super) fn served<'a>(request: &'a LaunchRequest, spare: Option<&Taken>) -> std::borrow::Cow<'a, LaunchRequest> {
    match spare.filter(|taken| taken.cwd != request.cwd) {
        Some(taken) => std::borrow::Cow::Owned(LaunchRequest { cwd: taken.cwd.clone(), ..request.clone() }),
        None => std::borrow::Cow::Borrowed(request),
    }
}
