//! The fleet registry: the `Fleet` handle, the ports it is built from, and the lookups every subject shares.
//!
//! Each subject lives beside this file: `inspecting` reads what the fleet holds, `prompting` drives a
//! session's turn, `starting` opens a session host, `streams` indexes a session's frames, `follower`
//! follows them into a transcript, and `books` reads what the ledger says.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};

use super::books::FleetBooks;
use super::claims::Claims;
use super::jobs::JobLeases;
use super::model::{Account, LaunchRequest, SessionRecord};
use super::promises::{Reckon, Unreckoned};
use super::records::read_record;
use super::run_recording::{NoRecording, RunRecording};
use super::runtime::Runtime;
use super::runtimes::Runtimes;
use super::store::{Registrar, SPOOL_DIR};
use super::usage_sink::{NoUsage, UsageSink};
pub use super::watch::TurnWatch;
use lanes::OverSshLanes;
pub use lanes::{RemoteLane, RemoteLanes};

mod adopting;
pub mod aside_launch;
mod awaiting;
mod claiming;
mod control;
mod depth;
pub mod efforts;
mod first_turn;
mod follower;
mod gate_commands;
mod gate_rows;
mod delivery;
mod hand_in;
mod handoff;
mod inheriting;
mod interactive;
mod kept;
mod lanes;
mod relaunching;
pub mod reserve;
pub use relaunching::Restart;
mod reopening;
mod retiring;
mod grounding;
mod launching;
mod home;
mod spares;
mod start_lock;
mod stopping;
mod withdrawing;
mod spend_rows;
mod spooled;
mod lead_grant;
mod takeover;
mod wait_rows;
mod table;
pub use home::{orders_here, recorded_here, retired_report_here, root_here};
pub use lanes::{RunningLane, worktree_of};
mod messages;
mod owners;
mod parents;
mod past;
mod senders;
mod status;
mod rebooting;
mod stranding;
mod proxy_window;
mod testing;
#[doc(hidden)]
pub mod fixtures;

mod books;
mod inspecting;
mod prompting;
mod starting;
mod streams;

pub use follower::Follower;
pub use home::roster;
use home::{fleet_root, home_dir, live_state_dir, state_dir};
pub use hand_in::HandedIn;
pub use control::Controlled;
pub use lead_grant::Granted;
pub use relaunching::{Choices, REVIVED_MID_TURN};
pub use stopping::Halt;
pub use gate_rows::Opened;
pub use takeover::backlog_line;
pub use messages::Sent;
pub use spend_rows::Spent;
pub use wait_rows::Waiting;
pub use proxy_window::{NoDesk, OpenedWindows, ProxyDesk};

#[derive(Clone)]
pub struct Fleet {
    pub(crate) root: PathBuf,
    state: PathBuf,
    home: PathBuf,
    policy: PathBuf,
    runtimes: Runtimes,
    ledger: Arc<dyn FleetBooks>,
    claims: Arc<dyn Claims>,
    host: String,
    arm: fn(i64) -> NorthResult<()>,
    registrar: Arc<dyn Registrar>,
    leases: Arc<dyn JobLeases>,
    proc: PathBuf,
    cgroups: PathBuf,
    admission: PathBuf,
    usage: Arc<dyn UsageSink>,
    recording: Arc<dyn RunRecording>,
    remote_lanes: Arc<dyn RemoteLanes>,
    desk: Arc<dyn ProxyDesk>,
    reckon: Arc<dyn Reckon>,
    spools_as: Option<String>,
    starts: spooled::Starts,
}

struct Ports {
    runtimes: Runtimes,
    ledger: Arc<dyn FleetBooks>,
    claims: Arc<dyn Claims>,
    registrar: Arc<dyn Registrar>,
    leases: Arc<dyn JobLeases>,
}

impl Fleet {
    pub fn open(runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>, registrar: Arc<dyn Registrar>, leases: Arc<dyn JobLeases>) -> NorthResult<Self> {
        let home = home_dir()?;
        let state = live_state_dir(&home);
        north_base::platform::land_queue::confined(&state).map_err(NorthError::Refused)?;
        let fleet = Self::rooted_at(home, state, crate::platform::orchestration::path(), Ports { runtimes, ledger, claims, registrar, leases })?;
        Ok(Self { spools_as: north_base::platform::session_scope::spooling_worker(), ..fleet })
    }

    fn rooted(home: PathBuf, policy: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>, registrar: Arc<dyn Registrar>, leases: Arc<dyn JobLeases>) -> NorthResult<Self> {
        let state = state_dir(&home);
        Self::rooted_at(home, state, policy, Ports { runtimes, ledger, claims, registrar, leases })
    }

    fn rooted_at(home: PathBuf, state: PathBuf, policy: PathBuf, ports: Ports) -> NorthResult<Self> {
        let Ports { runtimes, ledger, claims, registrar, leases } = ports;
        let root = fleet_root(&state);
        fs::create_dir_all(&root)?;
        let kept = root.join(lanes::KEPT_LANES);
        let fleet = Self { root, state, home, policy, runtimes, ledger, claims, host: crate::platform::host::name(), arm: parents::await_default, registrar, leases, proc: "/proc".into(), cgroups: "/sys/fs/cgroup".into(), admission: crate::platform::admission_root::here(), usage: Arc::new(NoUsage), recording: Arc::new(NoRecording), remote_lanes: Arc::new(OverSshLanes { kept }), desk: Arc::new(NoDesk), reckon: Arc::new(Unreckoned), spools_as: None, starts: spooled::start_spooled };
        fleet.adopt_journals()?;
        Ok(fleet)
    }

    pub fn writing_usage(mut self, usage: Arc<dyn UsageSink>) -> Self {
        self.usage = usage;
        self
    }

    pub fn recording_runs(mut self, recording: Arc<dyn RunRecording>) -> Self {
        self.recording = recording;
        self
    }

    pub fn reckoning_with(mut self, reckon: Arc<dyn Reckon>) -> Self {
        self.reckon = reckon;
        self
    }

    /// Shows the proxy on `desk`, the desktop Tom types on; a fleet without one starts no proxy.
    pub fn showing_proxy_on(mut self, desk: Arc<dyn ProxyDesk>) -> Self {
        self.desk = desk;
        self
    }

    pub(crate) fn account(&self, request: &LaunchRequest) -> NorthResult<Account> {
        self.runtimes.account(&state_dir(&self.home), &self.home, &self.policy, request)
    }

    fn runtime(&self, record: &SessionRecord) -> NorthResult<Arc<dyn Runtime>> {
        self.runtimes.get(&record.provider)
    }

    pub(crate) fn record(&self, name: &str) -> NorthResult<SessionRecord> {
        read_record(&self.session_dir(name)?).map_err(|_| NorthError::Usage(format!("no session named {name}")))
    }

    pub fn session_dir(&self, name: &str) -> NorthResult<PathBuf> {
        let valid = !name.is_empty() && !name.starts_with('.') && !name.contains('/') && name != SPOOL_DIR;
        if !valid {
            return Err(NorthError::Usage(format!("invalid session name {name:?}")));
        }
        Ok(self.root.join(name))
    }
}

fn session_running(record: &SessionRecord) -> bool {
    record.live()
}
