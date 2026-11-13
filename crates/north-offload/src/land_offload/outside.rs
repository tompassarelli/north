//! The two effects the offload asks its caller for: shipping a runtime this host holds to a build host that lacks it,
//! and running a worker's brokered request outside its Landlock domain.

use std::path::Path;

/// Ships a runtime this host holds to the host that lacks it, as the command layer supplies it: `north runtime`'s
/// deployment staging, which a run whose build host lacks the lander's runtime asks for.
pub type Stage = fn(host: &str, sha: &str, runtime: &Path) -> Result<(), String>;

/// Runs `north` outside a worker's Landlock domain, as the command layer supplies it; None outside one or when the
/// verb needs no way out.
pub type Reenter = fn(arguments: &[String]) -> Option<u8>;
