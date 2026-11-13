use std::path::Path;

use super::select::{Config, git_lines};

const GATES: &str = "harness/gates.toml";
const KEYS_GATE: &str = "safe-push-keys";

fn by_base(repo: &Path, base: &str, head: &str) -> bool {
    if base.is_empty() || git_lines(repo, &["diff", "--name-only", &format!("{base}...{head}"), "--", ".safe-push"]).is_empty() {
        return false;
    }
    let gates = git_lines(repo, &["show", &format!("{base}:{GATES}")]).join("\n");
    gates.parse::<toml_edit::DocumentMut>().is_ok_and(|gates| gates.get("gate").and_then(|gate| gate.get(KEYS_GATE)).and_then(|gate| gate.get("mode")).and_then(toml_edit::Item::as_str) == Some("enforce"))
}

/// The lines that judge the lane `base..head`: its own, or the landing base's when the lane changes `.safe-push` and `harness/gates.toml` at the base enforces `safe-push-keys`, so a lane cannot remove or weaken the check that would judge it (north#1287).
pub fn config(repo: &Path, base: &str, head: &str) -> Config {
    Config::load(repo, if by_base(repo, base, head) { base } else { head })
}
