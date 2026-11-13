use std::path::{Path, PathBuf};

pub const NAMING_FILE: &str = "/etc/north/agent-runtime-link";

pub const CLAUDE_LINK: &str = "agent-claude";

pub const OPERATOR_NORTH: &str = ".local/state/north/current/tree/bin/north";

pub const AGENT_POLICY_DIR: &str = "/etc/north/agents";

/// The operator-owned link beside the agent runtime link that names the store path of the Claude Code an agent session runs; `claude_script` execs `bin/claude` under it.
pub fn claude_link(agent_link: &Path) -> PathBuf {
    agent_link.parent().unwrap_or(Path::new("/")).join(CLAUDE_LINK)
}

pub fn claude_script() -> String {
    claude_script_naming(NAMING_FILE)
}

pub fn claude_script_naming(naming_file: &str) -> String {
    format!(r#"{} && exec "$(dirname -- "$f")/{CLAUDE_LINK}/bin/claude" "$@"; exit 127"#, named_link(naming_file))
}

/// Shell that sets `n` to the north program of the login it runs as: for the agent login only the agent runtime a root-owned naming file names, reading the policy the host keeps for the agent, so no program under the agent's own home answers for it; for the operator login its installed runtime.
pub fn login_north(agent: bool) -> String {
    login_north_naming(agent, NAMING_FILE)
}

fn login_north_naming(agent: bool, naming_file: &str) -> String {
    match agent {
        true => format!(r#"n=; {} && n="$f/north" && export AGENTS_OPERATOR_DIR={AGENT_POLICY_DIR}"#, named_link(naming_file)),
        false => format!(r#"n="$HOME/{OPERATOR_NORTH}""#),
    }
}

/// Shell that sets `f` to the first line of `naming_file` and succeeds only when that line is absolute and the file is root-owned and writable by no one else.
fn named_link(naming_file: &str) -> String {
    format!(r#"f=$(head -n 1 -- {naming_file} 2>/dev/null) && case "$f" in /*) ;; *) false;; esac && [ "$(stat -L -c %u -- {naming_file})" = 0 ] && m=$(stat -L -c %a -- {naming_file}) && case "$m" in *[2367]?|*[2367]) false;; esac"#)
}
