//! The launch of a provider CLI as a held unit on a build host, which a session's link attaches to and a warm spare waits in.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use north_base::platform::session_host_files::{BOUND_CONVERSATION, HELD};
use north_foundation::error::NorthResult;

use crate::machine::Machine;
use crate::ssh::quote;

pub const UNREGISTERED: Duration = Duration::from_secs(10 * 60);

/// `sh -c HOLD DIR UNIT DETACHED_SECS CLI...`: the service's main process. It feeds the CLI the
/// input file through a FIFO, records the CLI's status and stops the service once no link has been
/// attached for DETACHED_SECS, or for the seconds DIR/limit holds once a launch has written it.
pub const HOLD: &str = r#"d=$0 u=$1 limit=$2; shift 2
mkfifo "$d/feed" || exit 70
{ "$@" < "$d/feed" >> "$d/out" 2>> "$d/err"; echo $? > "$d/status"; } &
cli=$!
{ seen=$(date +%s); while sleep 60; do
  if read -r p b < "$d/attach" && [ "$(cut -d' ' -f22 "/proc/$p/stat")" = "$b" ]; then seen=$(date +%s)
  elif [ $(($(date +%s) - seen)) -ge "$(cat "$d/limit" 2>/dev/null || echo "$limit")" ]; then systemctl --user stop "$u"; fi
done; } 2>/dev/null &
watch=$!
tail -c +1 -f --pid="$cli" "$d/in" > "$d/feed"
wait "$cli"; code=$?
kill "$watch" 2>/dev/null
exit "$code""#;

/// `sh -c PRUNE HELD UNIT`: removes held directories untouched for three days whose service is gone, other than UNIT's, which a launch beside it is making.
pub const PRUNE: &str = r#"for h in "$0"/*.service; do
  [ "${h##*/}" != "$1" ] && [ -d "$h" ] && [ -z "$(find "$h" -maxdepth 1 -mmin -4320 -print -quit)" ] && ! systemctl --user -q is-active "${h##*/}" &&
    { [ ! -d "$h/bound" ] || { chmod 700 "$h/bound" && rm -f "$h/bound/binding" && rmdir "$h/bound"; }; } &&
    rm -f "$h/in" "$h/out" "$h/err" "$h/status" "$h/attach" "$h/feed" "$h/nonce" "$h/limit" "$h/retire" "$h/conversation" && rmdir "$h"
done"#;

/// What a profile file holds where a session would run, as a launch check sees it: its text, nothing there, or a machine that did not answer, which the launch itself reports.
#[derive(Clone)]
pub enum Seen {
    Text(String),
    Absent,
    Unreachable,
}

pub struct HostSpec {
    pub key: String,
    pub shared: bool,
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
}

/// A held unit already running on another machine: its name, its directory and the nonce a link attaches with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Running {
    pub unit: String,
    pub dir: String,
    pub nonce: String,
}

/// The held unit a launch starts: its name, its directory, the nonce a link attaches with and the conversation it is bound to, or None to leave it bound to none.
pub struct Starting<'a> {
    pub unit: &'a str,
    pub dir: &'a str,
    pub nonce: &'a str,
    pub conversation: Option<&'a str>,
}

pub fn new_uuid() -> NorthResult<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned())
}

/// The spec `machine` starts a session from: its working directory moved under the session login's own home, so an agent login stages in the checkout it reads and never in the operator's.
pub fn staged_on(spec: HostSpec, machine: &Machine, home: &Path) -> HostSpec {
    HostSpec { cwd: machine.session_path(&spec.cwd, home), ..spec }
}

/// The held directory of the unit `unit` under `home`.
pub fn held_dir(home: &Path, unit: &str) -> PathBuf {
    home.join(HELD).join(unit)
}

/// The script that starts `spec`'s CLI as the held unit `starting` under `held`, stopped once no link has been attached for `detached`, with `properties` added to the unit.
pub fn launch_script(spec: &HostSpec, held: &Path, starting: &Starting, detached: Duration, properties: &[String]) -> String {
    let Starting { unit, dir, nonce, conversation } = *starting;
    let (q_unit, q_dir) = (quote(unit), quote(dir));
    let binding = conversation.map_or_else(|| format!(" && rm -f {BOUND_CONVERSATION}"), |conversation| format!(" && printf %s {} > {BOUND_CONVERSATION}.launch && mv {BOUND_CONVERSATION}.launch {BOUND_CONVERSATION}", quote(conversation)));
    let mut launch = format!(
        "sh -c {} {} {q_unit} </dev/null >/dev/null 2>&1 & systemctl --user stop {q_unit} 2>/dev/null; mkdir -p {q_dir} && cd {q_dir} && rm -f in out err status attach feed nonce binding limit && : > in && : > out && : > err && printf %s {} > nonce{binding} && cd {} && systemd-run --user --quiet --collect --same-dir --slice=agent.slice --unit={q_unit} --property=KillMode=control-group --property=IOAccounting=yes --expand-environment=no \"--setenv=PATH=$PATH\"",
        quote(PRUNE),
        quote(&held.to_string_lossy()),
        quote(nonce),
        quote(&spec.cwd.to_string_lossy())
    );
    for property in properties {
        launch.push(' ');
        launch.push_str(&quote(&format!("--property={property}")));
    }
    for (key, value) in &spec.env {
        launch.push(' ');
        launch.push_str(&quote(&format!("--setenv={key}={}", value.to_string_lossy())));
    }
    launch.push_str(&format!(" -- sh -c {} {q_dir} {q_unit} {}", quote(HOLD), detached.as_secs()));
    for arg in &spec.argv {
        launch.push(' ');
        launch.push_str(&quote(arg));
    }
    launch
}
