//! Starting a session's host: the host it opens, the record it writes live, and the frames it sends first.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::claims;
use crate::fleet::host::Host;
use crate::fleet::hops;
use crate::fleet::model::{Account, SessionRecord};
use crate::fleet::prompt::{PromptInputs, prompt_sha, render};
use crate::fleet::records::{CLAIMS_RELEASED, PLACED, write_record};
use crate::fleet::remote::{self, HooksCheck};
use crate::fleet::runtime::{DEFAULT_FOREGROUND_CAP_SECS, Instructions, Opening, Runtime, now_ms};
use crate::fleet::spares::Taken;
use crate::fleet::transcript::{session_stream_path, stream_len};
use crate::fleet::unaccepted::Unaccepted;

use super::awaiting::await_conversation;
use super::{Fleet, session_running};

const DEFAULT_IDLE_EXIT_MINUTES: u64 = 10;
const HOSTS: &str = ".hosts";

impl Fleet {
    pub(super) fn start(&self, record: &mut SessionRecord, account: &Account, opening: Opening, hooks: HooksCheck) -> NorthResult<()> {
        let opened = self.open_host(record, account, opening, None)?;
        self.go_live(record, opening, hooks, opened)
    }

    /// Starts the session's host, and with it the provider CLI, or joins the shared host it runs in; the session is not yet recorded live, so a launch runs this beside its registration and sends nothing but the CLI's handshake until that is in. With `spare` taken on the session's machine, its link attaches to that client instead of starting one, and the spare is stopped when the host does not start.
    pub(super) fn open_host(&self, record: &SessionRecord, account: &Account, opening: Opening, spare: Option<&Taken>) -> NorthResult<OpenedHost> {
        let opened = self.open_host_from(record, account, opening, spare);
        if let (Err(_), Some(taken), Some(machine)) = (&opened, spare, &record.machine) {
            self.release_spare(machine, taken);
        }
        opened
    }

    fn open_host_from(&self, record: &SessionRecord, account: &Account, opening: Opening, spare: Option<&Taken>) -> NorthResult<OpenedHost> {
        let since_ms = if record.host_dir.as_os_str().is_empty() { record.created_ms } else { now_ms() };
        self.refuse_on_window(record.conversation.as_deref())?;
        let runtime = self.runtime(record)?;
        self.refuse_headless_proxy(record, runtime.as_ref(), account)?;
        self.refuse_reserve(record, account)?;
        let (instructions, sha) = self.instructions(record)?;
        let trace = hops::trace_in(&self.root.join(&record.name));
        let spec = runtime.host(account, &record.relaunch_request(), opening, &instructions)?;
        let spec = match (spare, &record.machine) {
            (Some(taken), Some(machine)) => remote::attached(spec, machine, &taken.running, &record.name, &trace)?,
            _ => remote::placed(spec, record, &trace)?,
        };
        let host_dir = if spec.shared { self.root.join(HOSTS).join(&spec.key) } else { self.root.join(&record.name) };
        let host = Host::new(host_dir.clone());
        let started = !host.running();
        if started {
            host.start(&spec, &record.provider, idle_exit(record), &account.recorded_label())?;
            host.send(&runtime.handshake())?;
        }
        let held_unit = spare.map(|taken| taken.running.unit.clone());
        Ok(OpenedHost { runtime, instructions, sha, trace, host_dir, host, started, since_ms, held_unit })
    }

    pub(super) fn go_live(&self, record: &mut SessionRecord, opening: Opening, hooks: HooksCheck, opened: OpenedHost) -> NorthResult<()> {
        let OpenedHost { runtime, instructions, sha, trace, host_dir, host, started, since_ms, held_unit } = opened;
        record.held_unit = held_unit;
        let request = record.relaunch_request();
        let changed = record.system_prompt_sha.as_deref() != Some(sha.as_str());
        record.system_prompt_sha = Some(sha);
        if let Some(reason) = hooks.verdict()? {
            if started {
                host.terminate()?;
            }
            return Err(NorthError::Refused(reason));
        }
        if record.host_dir != host_dir {
            let _ = fs::remove_file(session_stream_path(&host_dir, &record.name));
            record.stream_offset = stream_len(&host_dir);
            record.host_dir = host_dir.clone();
        }
        record.host_pid = host.pid();
        write_record(&self.root.join(&record.name), record)?;
        let cause = if started { "a session host started for it" } else { "it joined its running session host" };
        let phase = self.phases()?.live_with(record.launch(), record.host_pid, cause, &Self::watchdog(record))?;
        hops::record(&trace, &hops::Span { session: record.name.clone(), hop: hops::Hop::SessionHost, start_ms: since_ms, end_ms: now_ms() });
        record.phase = Some(phase);
        match fs::remove_file(self.root.join(&record.name).join(CLAIMS_RELEASED)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        self.start_watchdogs()?;
        let frames = runtime.open(&record.name, &request, opening, Instructions { text: &instructions, changed })?;
        if !frames.is_empty() {
            let opened_from = stream_len(&host_dir);
            host.send(&frames)?;
            record.conversation = Some(await_conversation(runtime.as_ref(), record, opened_from)?);
            write_record(&self.root.join(&record.name), record)?;
        }
        if started {
            host.send(&Unaccepted::to_replay(&host_dir))?;
        }
        remote::leave_record(record, &self.root.join(PLACED));
        claims::hold_for(self.claims.as_ref(), record, &self.ancestors(record.parent.as_deref()));
        self.show_proxy(record, &host)?;
        Ok(())
    }

    pub(super) fn instructions(&self, record: &SessionRecord) -> NorthResult<(String, String)> {
        let orders = self.orders().read()?;
        let extra = record.system_prompt.as_deref().map(|path| fs::read_to_string(path).map_err(|error| NorthError::io_at(error, || format!("reading {}", path.display())))).transpose()?;
        let inputs = PromptInputs {
            role: record.role(),
            mode: record.prompt_mode,
            orders: &orders,
            foreground_cap_secs: record.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS),
            extra: extra.as_deref(),
        };
        let text = render(&inputs);
        let sha = prompt_sha(&text);
        Ok((text, sha))
    }

    pub(super) fn host_in_use(&self, stopping: &SessionRecord) -> NorthResult<bool> {
        Ok(self.sessions()?.iter().any(|other| {
            other.name != stopping.name && other.host_dir == stopping.host_dir && session_running(other)
        }))
    }
}

/// A session host a start has running, or joined, and has not yet recorded live.
pub(super) struct OpenedHost {
    runtime: Arc<dyn Runtime>,
    instructions: String,
    sha: String,
    trace: PathBuf,
    host_dir: PathBuf,
    host: Host,
    started: bool,
    since_ms: u64,
    held_unit: Option<String>,
}

impl OpenedHost {
    pub(super) fn abandon(self) -> NorthResult<()> {
        if self.started { self.host.terminate() } else { Ok(()) }
    }
}

pub(super) fn idle_exit(record: &SessionRecord) -> Option<Duration> {
    let minutes = record.idle_exit_minutes.unwrap_or(DEFAULT_IDLE_EXIT_MINUTES);
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}
