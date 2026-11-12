use super::commands::{self, OWN_UNIT, TERMINAL_UNIT};
use super::{HOST_RESTART, Hit};

pub(super) fn reason(hits: &[Hit]) -> String {
    hits.iter().map(hit_reason).collect::<Vec<_>>().join("\n")
}

fn hit_reason(hit: &Hit) -> String {
    match hit {
        Hit::Teardown { shape, host: None } if commands::UNIT_SHAPES.contains(shape) => unit_reason(shape),
        Hit::Teardown {
            shape,
            host: Some(host),
        } => format!(
            "BLOCKED: {shape} on {host} over ssh — this stops every unit, farm runner, container and agent session that host's user manager runs. Run `{HOST_RESTART} {host}` instead: it restarts the user manager only after the host is idle (no busy farm runner job, no fleet session placed there, no running containers, no LAN pool units) and otherwise refuses, naming what blocks. Remote `systemctl --user status|list-units|show` stays allowed."
        ),
        Hit::Teardown { shape, host: None } => format!(
            "BLOCKED: session-killing shape refused ({shape}) — this signals the whole desktop session (compositor, user manager, every agent), not just your target. Signal only processes YOU started, by a recorded PID (`kill <pid>`), a pid file (`pkill -F pidfile`) or your own cgroup (`pkill --cgroup CGROUP PATTERN`); manage your own units by exact name. To restart a host's user manager, run `{HOST_RESTART} HOST`, which refuses while the host is in use. Session/compositor teardown is the operator's call. Prose that mentions these phrases is unaffected — only a command-position invocation is denied. Rare explicit override: `north config agents off session-kill-guard` (persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
        ),
        Hit::SelfMatch { tool, pattern } => format!(
            "BLOCKED: self-matching `{tool} -f {pattern}` — the pattern also matches the command line this call runs in (the shell running it carries the same text), so it finds and signals its own shell and whatever else runs in it. Write the pattern in bracket form so the pattern text no longer matches itself (`{tool} -f '{bracket}'`, e.g. `'[c]argo test'`), add `-A` to ignore ancestors, or avoid pattern matching: record the pid in a pid file when you start the process and use `kill $(cat file.pid)` or `{tool} -F file.pid`.",
            bracket = bracketed(pattern)
        ),
        Hit::CrossSession { tool, operands } => format!(
            "BLOCKED: cross-session `{tool} {operands}` — a pattern or process name matches every session's processes on this machine, not just yours (bracket form only stops a pattern matching itself), and such a kill already ended the proxy's landing waiter and every session's cargo test. Signal only what you started: `pkill --cgroup CGROUP PATTERN` scoped to your own cgroup from /proc/self/cgroup, `kill PID` with a recorded pid you saved when you started the process, or `pkill -F pidfile` with a pid file you wrote."
        ),
        Hit::Detached(shape) => format!(
            "BLOCKED: {shape} would detach an agent child from its owner. Use `north capacity bounded <duration> -- <command>`; the wrapper has a 24h maximum, owns a transient cgroup plus PID namespace, reaps every descendant when the owner ends, and gives all background jobs a shared 48G hard ceiling. A bounded or run_in_background job ends with this session; for a command that must outlive it use `north job-run --notify NAME -- <command>`, which runs it in its own unit and messages this session when it exits."
        ),
    }
}

fn unit_reason(shape: &str) -> String {
    let why = match shape {
        TERMINAL_UNIT => "this unit holds a terminal window or desktop app, so stopping it closes Tom's terminal",
        OWN_UNIT => "this unit is on your own cgroup path, so stopping it tears down the terminal scope this session runs in",
        _ => "its operand comes from `$VAR`, `$(...)`, backticks or an xargs list the guard cannot see, and that kind of lookup already closed Tom's terminal scope, so a fixed suffix naming your own units does not make it safe",
    };
    format!(
        "BLOCKED: {shape} refused — {why}. Stop only a unit you launched yourself by name (`systemd-run --user --unit=NAME ...`, then `systemctl --user stop NAME`), and first verify it is not on your own cgroup path in /proc/self/cgroup."
    )
}

fn bracketed(pattern: &str) -> String {
    let mut chars = pattern.chars();
    match chars.next() {
        Some(first) if first.is_alphanumeric() => format!("[{first}]{}", chars.as_str()),
        _ => pattern.to_owned(),
    }
}
