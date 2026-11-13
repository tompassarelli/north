use std::env;
use std::path::{Path, PathBuf};

pub const ROOT_ENV: &str = "NORTH_OFFLOAD_ROOT";
pub const ROOT_DEFAULT: &str = ".cache/land-offload";
/// Where, under the home of a build host's user, the offload's script records the checkout each slot it takes belongs to: one file a slot, named as the slot is, holding that checkout's path under the home. No worker command writes there, so the sandbox broker reads from it which project a slot is.
pub const SLOT_RECORDS: &str = ".local/state/north/offload-slots";
pub const OWNER_ENV: &str = "NORTH_OFFLOAD_OWNER";
pub const RED_FIX_ENV: &str = "NORTH_LAND_RED_FIX";
pub const LANDER_SLOT_ENV: &str = "NORTH_OFFLOAD_LANDER_SLOT";
pub const URGENT_ENV: &str = "NORTH_OFFLOAD_URGENT";
pub const SLOT_ENV: &str = "NORTH_OFFLOAD_SLOT";
/// Set only on the checks before queueing, whose caller answers a refusal as a reason to place the lane again: it opts the run in to the duplicate-job probe, and names the build hosts the run already failed on, space-separated, which the probe leaves out because a failed attempt may still hold its marker. A run without it is never refused as a twin, since only that caller knows the refusal.
pub const TWIN_AWAY_ENV: &str = "NORTH_OFFLOAD_TWIN_AWAY";
/// The script file an offload run hands its leased shell, which admission charges and records as the check it runs rather than as its wrapper.
pub const SUBJECT_ENV: &str = "NORTH_OFFLOAD_SUBJECT";
/// The name the directory of that script file begins with, before the nonce `mktemp` gives it: the offload script makes the directory for the one file, and admission reads a subject from no other place.
pub const SUBJECT_DIRECTORY: &str = "offload-command.";
/// The name of that script file in its directory.
pub const SUBJECT_FILE: &str = "command";
/// The name, in the user's runtime directory, every offload master socket begins with: the offload rides it and the worker sandbox hides it, so only the brokered offload verbs reach a build host from a worker command (south#118).
pub const MASTER_SOCKET: &str = "north-offload-";
/// The pid of the shell that holds the slot `SLOT_ENV` names, whose life is the life of its hold, so a job nested in that slot ends when it ends.
pub const SLOT_HOLDER_ENV: &str = "NORTH_OFFLOAD_SLOT_HOLDER";
/// The line an offload run prints for people once it proved its slot holds exactly the shipped commit's tree, followed by that tree and `of COMMIT`; nothing decides on it, since a check's own output can print it too.
pub const PROVED_TREE: &str = "offload: proved tree ";
/// The line an offload run prints once its slot holds the proved tree, followed by what each of its steps took since the one before, as `NAME=MILLISECONDS` words: receiving the bundle, filling the checkout, taking a slot (its line wait included), placing the slot on the commit and proving its tree; the lander books them as the parts of the job's slot and tree proof.
pub const TIMED: &str = "offload: timed";
/// The file an after-landing run's offload clients append one line to for each shipped run whose command reported: the tree the build host proved the run was computed on, else `UNPROVED`, then the commit; the run's log never names it, so the sheriff reads this.
pub const TREES_ENV: &str = "NORTH_OFFLOAD_TREES";
pub const UNPROVED: &str = "unproved";
pub const LANES_ENV: &str = "NORTH_LAND_LANES";
pub const TIPS_ENV: &str = "SAFE_PUSH_LAND_TIPS";
pub const ONE_LANE_ENV: &str = "SAFE_PUSH_LAND_ONE_LANE";
pub const LOCAL_ENV: &str = "NORTH_LAND_CHECKS_LOCAL";
pub const OFF_HOST_ENV: &str = "NORTH_LAND_CHECKS_OFF_HOST";
/// The longest a land check runs, the ceiling of its deadline, and so the longest run a worker command may ask the outside broker for.
pub const RUN_SECONDS_CEILING: u64 = 2100;
pub const LAND_LINE_DEADLINE_SECONDS: u64 = 600;
/// The ssh and the git an offload starts, as absolute programs: the outside broker resolves them from its own `PATH` and names them here for the run it starts for a worker command, so no program on a `PATH` the worker wrote runs outside its domain (south#118).
pub const SSH_ENV: &str = "NORTH_OFFLOAD_SSH";
pub const GIT_ENV: &str = "NORTH_OFFLOAD_GIT";
pub const WORKER_REQUEST_ENV: &str = "NORTH_OFFLOAD_WORKER_REQUEST";

/// The ssh an offload starts: the absolute program `SSH_ENV` names, else `ssh` by name.
pub fn ssh() -> String {
    program(SSH_ENV, "ssh")
}

/// The git an offload starts: the absolute program `GIT_ENV` names, else `git` by name.
pub fn git() -> String {
    program(GIT_ENV, "git")
}

fn program(variable: &str, name: &str) -> String {
    env::var(variable).ok().filter(|program| Path::new(program).is_absolute()).unwrap_or_else(|| name.to_owned())
}

pub fn root() -> Option<PathBuf> {
    env::var_os(ROOT_ENV)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").filter(|home| !home.is_empty()).map(|home| PathBuf::from(home).join(ROOT_DEFAULT)))
}

/// The shell function `slot_held LOCK`: whether a job holds the offload slot whose lock is `LOCK`. It tests the shared lock every holder keeps
/// on the slot's `.busy` file and never the `.lock` itself, because a test that took the `.lock` for an instant would fail a job's `flock -n` take of the slot in that instant.
pub const SLOT_HELD_SH: &str = r#"slot_held() {
  busy=${1%.lock}.busy
  [ -e "$busy" ] && ! (exec 7<"$busy"; flock -n 7) 2>/dev/null
}
"#;
