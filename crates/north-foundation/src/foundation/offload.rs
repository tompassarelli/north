use std::env;
use std::path::PathBuf;

pub const ROOT_ENV: &str = "NORTH_OFFLOAD_ROOT";
pub const ROOT_DEFAULT: &str = ".cache/land-offload";
pub const OWNER_ENV: &str = "NORTH_OFFLOAD_OWNER";
pub const RED_FIX_ENV: &str = "NORTH_LAND_RED_FIX";
pub const LANDER_SLOT_ENV: &str = "NORTH_OFFLOAD_LANDER_SLOT";
pub const SLOT_ENV: &str = "NORTH_OFFLOAD_SLOT";
/// The line an offload run prints once it proved its slot holds exactly the shipped commit's tree, followed by that tree and `of COMMIT`, which an after-landing verdict reads to name the tree it was computed on.
pub const PROVED_TREE: &str = "offload: proved tree ";
pub const LANES_ENV: &str = "NORTH_LAND_LANES";
pub const TIPS_ENV: &str = "SAFE_PUSH_LAND_TIPS";
pub const LOCAL_ENV: &str = "NORTH_LAND_CHECKS_LOCAL";
pub const OFF_HOST_ENV: &str = "NORTH_LAND_CHECKS_OFF_HOST";

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
