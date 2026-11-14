use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine as _;
use north_trusted_mirror::Mirror;

use super::after::route::{self, Outbox, SSH_OPTIONS};
use super::candidate::git;
use super::lander::{Lander, OFFLOAD_OWNER, plain};
use super::queue::{self, Ticket};
use north_base::land_ticket::Records;
use north_base::land_ticket::{self, Phase};

const WAIT_TICK: Duration = Duration::from_millis(200);
const BUNDLE_LIMIT: usize = 8 << 20;

fn ssh(host: &str, script: &str) -> Result<String, String> {
    ssh_with_input(host, script, b"")
}

fn ssh_with_input(host: &str, script: &str, input: &[u8]) -> Result<String, String> {
    let mut child = Command::new("ssh").args(SSH_OPTIONS).args(["-T", host, "--", script]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("ssh {host}: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    let output = child.wait_with_output().map_err(|error| format!("ssh {host}: {error}"))?;
    if !output.status.success() {
        return Err(format!("ssh {host} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn lane_ref(outbox: &Outbox, branch: &str) -> String {
    format!("refs/land-remote/{}/{}", outbox.name(), queue::slug(branch))
}

fn fetch_lane_once(store: &Path, outbox: &Outbox, ticket: &Ticket) -> Result<(), String> {
    let url = format!("ssh://{}{}", outbox.login, ticket.root.display());
    let target = format!("+refs/heads/{}:{}", ticket.branch, lane_ref(outbox, &ticket.branch));
    let ssh_command = std::iter::once("ssh").chain(SSH_OPTIONS.iter().copied()).collect::<Vec<_>>().join(" ");
    let output = north_trusted_mirror::command(store).args(["fetch", "-q", "--no-tags", &url, &target]).env("GIT_SSH_COMMAND", ssh_command).output().map_err(|error| format!("git fetch {url}: {error}"))?;
    if !output.status.success() {
        return Err(format!("git fetch {url} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

enum Refusal {
    Unreachable(String),
    Moved(String),
}

impl Lander {
    fn fetch_lane(&self, outbox: &Outbox, ticket: &Ticket) -> Result<(), String> {
        let store = self.lane_store()?;
        north_git_locks::retry_after_clearing(&store, || fetch_lane_once(&store, outbox, ticket))
    }

    fn lane_store(&self) -> Result<std::path::PathBuf, String> {
        Ok(Mirror::open(&super::repositories::checkout(&self.repo)?)?.git_dir().to_owned())
    }

    fn remote_queue(&self) -> Result<String, String> {
        let key = self.queue.file_name().and_then(|name| name.to_str()).ok_or_else(|| format!("{} has no queue key", self.queue.display()))?;
        Ok(format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/land-queue/{key}\""))
    }

    fn remote_outbox(&self) -> Result<String, String> {
        Ok(format!("{}/{}", self.remote_queue()?, queue::OUTBOX))
    }

    /// The outbox host a ticket was handed in on: the remote host named like the ticket's host, none for this host's own.
    pub fn outbox_host(&self, ticket: &Ticket) -> Option<String> {
        let named = ticket.host.split('.').next().filter(|name| !name.is_empty())?;
        self.remote_hosts().into_iter().find(|host| host.split('.').next() == Some(named) && plain(host))
    }

    /// Writes `text` as `AREA/NAME` in the outbox `login` reads for this queue, where the hand-in waiting on that ticket reads it.
    pub fn tell(&self, login: &str, area: &str, name: &str, text: &str) -> Result<(), String> {
        if !plain(name) || !plain(area) {
            return Err(format!("{name:?} in {area:?} is not a ticket name"));
        }
        let dir = self.remote_outbox()?;
        ssh_with_input(login, &format!("d={dir}/{area};mkdir -p \"$d\" && cat >\"$d/.{name}.part\" && mv -f \"$d/.{name}.part\" \"$d/{name}\""), text.as_bytes()).map(drop)
    }

    /// Sends the decided ticket at `path` to the outbox host it was handed in on, where its waiting hand-in reads the
    /// verdict at once, through the login that owns its lane, and names that host; a ticket handed in on this host needs no delivery.
    pub fn deliver(&self, path: &Path) -> Result<Option<String>, String> {
        let ticket = queue::read(path).map_err(|error| error.to_string())?;
        let (Some(host), Some(name), Some(_)) = (self.outbox_host(&ticket), path.file_name().and_then(|name| name.to_str()), ticket.verdict.as_ref()) else { return Ok(None) };
        let text = serde_json::to_string_pretty(&ticket).map_err(|error| error.to_string())?;
        self.tell(&route::lane_login(&host, &ticket.root)?, queue::DECIDED, name, &format!("{text}\n")).map(|()| Some(host))
    }

    /// Answers a waiting hand-in's ask about ticket `name`, filed from `outbox`'s own host and user: its decided ticket when it has one, else where it stands here; a ticket filed from any other outbox is answered as unknown.
    fn answer(&self, outbox: &Outbox, name: &str) -> Result<(), String> {
        let decided = land_ticket::directories(&self.queue).into_iter().skip(1).map(|area| area.join(name)).find(|path| path.is_file());
        let waiting = decided.clone().unwrap_or_else(|| self.queue.join(name));
        let here = crate::platform::land_queue::local_host();
        let text = match queue::read(&waiting).ok().filter(|ticket| astray(outbox, ticket).is_none()) {
            Some(_) if decided.is_some() => return self.deliver(&waiting).map(drop),
            Some(ticket) => {
                let checking = Records::open(&self.queue).ok().and_then(|records| records.phase(&waiting)) == Some(Phase::Checking);
                let place = if checking { "being checked".to_owned() } else { queue::queue_position(&self.queue, &waiting).ok().flatten().map_or_else(|| "waiting".to_owned(), |(place, total)| format!("{place} of {total} queued")) };
                let deferral = ticket.deferral.as_deref().filter(|reason| !reason.trim().is_empty()).map(|reason| format!("; {reason}")).unwrap_or_default();
                format!("the lander on {here} has {} ({}) {place}{deferral}", ticket.branch, ticket.head)
            }
            None => format!("the lander on {here} holds no ticket {name}"),
        };
        self.tell(&outbox.login, queue::ANSWERS, name, &format!("{text}\n"))
    }

    pub fn pull_host(&self, outbox: &Outbox, wait: Duration, taken: &mut Vec<String>) -> Result<usize, String> {
        let host = outbox.host.as_str();
        let home = self.remote_queue()?;
        let dir = self.remote_outbox()?;
        let mirror = if outbox.agent { String::new() } else { format!("q={home}; mkdir -p \"$q\" && cat >\"$q/{0}.part\" && mv -f \"$q/{0}.part\" \"$q/{0}\" || exit 1; ", queue::LANDER_LEDGER) };
        let ticks = wait.as_millis() / WAIT_TICK.as_millis();
        let ledger = if outbox.agent { String::new() } else { ledger_tail() };
        let settle = settle_script(taken);
        let asks = format!("for f in \"$d\"/{}/*.json; do [ -f \"$f\" ] || continue; printf '?%s\\n\\000' \"${{f##*/}}\"; rm -f -- \"$f\"; done; ", queue::ASKS);
        let bundle = format!("b=\"${{f%.json}}.{}\"; [ -f \"$b\" ] && {{ printf '=%s\\n' \"${{f##*/}}\"; head -c {} \"$b\" | base64 -w0; printf '\\000'; }}; ", queue::BUNDLE, BUNDLE_LIMIT + 1);
        let listing = ssh_with_input(&outbox.login, &format!("d={dir}; {settle}n={ticks}; while [ \"$n\" -gt 0 ]; do for f in \"$d\"/*.json \"$d\"/{0}/*.json; do [ -f \"$f\" ] && break 2; done; n=$((n-1)); sleep {tick}; done; {asks}set -- \"$d\"/*.json; [ -e \"$1\" ] || exit 0; {mirror} for f in \"$d\"/*.json; do [ -f \"$f\" ] || continue; {bundle}printf '%s\\n' \"${{f##*/}}\"; cat \"$f\"; printf '\\000'; done", queue::ASKS, tick = WAIT_TICK.as_secs_f32()), ledger.as_bytes())?;
        taken.clear();
        let mut records = Records::open(&self.queue).map_err(|error| format!("cannot read the records of {}: {error}", self.queue.display()))?;
        let mut failures = Vec::new();
        let mut withdrawn = Vec::new();
        let mut asked = Vec::new();
        let mut bundles = HashMap::new();
        let mut arrived = Vec::new();
        for record in listing.split('\0').filter(|record| !record.trim().is_empty()) {
            let (name, json) = record.split_once('\n').unwrap_or((record, ""));
            if let Some(name) = name.strip_prefix('?') {
                asked.push(name.to_owned());
                continue;
            }
            if let Some(name) = name.strip_prefix('=') {
                bundles.insert(name.to_owned(), json.to_owned());
                continue;
            }
            if !plain(name) || !name.ends_with(".json") {
                failures.push(format!("{name:?} is not a ticket name"));
                continue;
            }
            let ticket: Ticket = match serde_json::from_str(json) {
                Ok(ticket) => ticket,
                Err(error) => {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            };
            if let Some(reason) = astray(outbox, &ticket) {
                withdrawn.push((name.to_owned(), ticket, reason));
                continue;
            }
            match self.take_lane(outbox, &ticket, bundles.remove(name).as_deref()) {
                Ok(()) => arrived.push((name.to_owned(), ticket)),
                Err(Refusal::Moved(reason)) => withdrawn.push((name.to_owned(), ticket, reason)),
                Err(Refusal::Unreachable(_)) if lane_gone(outbox, &ticket) => {
                    let reason = format!("its worktree {} is gone from {host}, so the lander cannot fetch {}", ticket.root.display(), ticket.head);
                    withdrawn.push((name.to_owned(), ticket, reason));
                }
                Err(Refusal::Unreachable(error)) => failures.push(error),
            }
        }
        for (name, ticket) in arrived {
            let path = self.queue.join(&name);
            if records.phase(&path).is_none() {
                queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                records.moved(&path, Phase::Queued, &format!("taken from {host}'s outbox")).map_err(|error| format!("cannot record {}: {error}", path.display()))?;
            }
            taken.push(name);
        }
        if !withdrawn.is_empty() {
            self.withdraw(outbox, &dir, withdrawn)?;
        }
        failures.extend(asked.iter().filter_map(|name| self.answer(outbox, name).err().map(|error| format!("cannot answer {host}'s ask about {name}: {error}"))));
        if failures.is_empty() { Ok(taken.len()) } else { Err(failures.join("; ")) }
    }

    fn take_lane(&self, outbox: &Outbox, ticket: &Ticket, bundle: Option<&str>) -> Result<(), Refusal> {
        self.fetch_into_repo(outbox, ticket, bundle)?;
        let checkout = super::repositories::checkout(&self.repo).map_err(Refusal::Unreachable)?;
        Mirror::open(&checkout).and_then(|mirror| mirror.publish_lane(&self.repo, &ticket.branch, &ticket.head)).map_err(Refusal::Unreachable)
    }

    fn fetch_into_repo(&self, outbox: &Outbox, ticket: &Ticket, bundle: Option<&str>) -> Result<(), Refusal> {
        let host = &outbox.host;
        let from_bundle = bundle.map(|bundle| self.unbundle(outbox, ticket, bundle));
        if from_bundle.as_ref().is_some_and(Result::is_ok) && self.holds(outbox, ticket) {
            println!("north land: took {} from {host}'s bundle", ticket.branch);
            return Ok(());
        }
        match from_bundle {
            Some(Err(error)) => println!("north land: fetching {} from {host}, since the bundle its ticket carried did not apply: {error}", ticket.branch),
            Some(Ok(())) => println!("north land: fetching {} from {host}, since the bundle its ticket carried does not hold {}", ticket.branch, ticket.head),
            None => {}
        }
        self.fetch_lane(outbox, ticket).map_err(Refusal::Unreachable)?;
        if self.holds(outbox, ticket) { Ok(()) } else { Err(Refusal::Moved(format!("{} on {host} no longer holds {}, so the lander cannot take it", ticket.branch, ticket.head))) }
    }

    fn holds(&self, outbox: &Outbox, ticket: &Ticket) -> bool {
        let head = &ticket.head;
        !head.is_empty() && head.chars().all(|digit| digit.is_ascii_hexdigit()) && self.lane_store().is_ok_and(|store| git(&store, &["merge-base", "--is-ancestor", head, &lane_ref(outbox, &ticket.branch)]).is_ok())
    }

    fn unbundle(&self, outbox: &Outbox, ticket: &Ticket, encoded: &str) -> Result<(), String> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|error| format!("the bundle is not base64: {error}"))?;
        if bytes.len() > BUNDLE_LIMIT {
            return Err(format!("the bundle is over {BUNDLE_LIMIT} bytes"));
        }
        let mut file = tempfile::Builder::new().suffix(".bundle").tempfile().map_err(|error| format!("cannot hold the bundle: {error}"))?;
        file.write_all(&bytes).map_err(|error| format!("cannot hold the bundle: {error}"))?;
        let path = file.path().to_str().ok_or("the bundle's path is not UTF-8")?.to_owned();
        let target = format!("+refs/heads/{}:{}", ticket.branch, lane_ref(outbox, &ticket.branch));
        let store = self.lane_store()?;
        north_git_locks::retry_after_clearing(&store, || git(&store, &["fetch", "-q", "--no-tags", &path, &target]).map(drop))
    }

    fn withdraw(&self, outbox: &Outbox, dir: &str, withdrawn: Vec<(String, Ticket, String)>) -> Result<(), String> {
        let host = &outbox.host;
        let mut told = Vec::new();
        for (name, mut ticket, reason) in withdrawn {
            ticket.verdict = Some(format!("withdrawn by {OFFLOAD_OWNER}: {reason}"));
            let path = self.queue.join(queue::WITHDRAWN).join(&name);
            Records::open(&self.queue).and_then(|mut records| records.moved(&path, Phase::Withdrawn, ticket.verdict.as_deref().unwrap_or_default())).map_err(|error| format!("cannot record {}: {error}", path.display()))?;
            queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: withdrew {name} from {host}'s outbox: {reason}");
            told.push((name, ticket, reason));
        }
        let names: Vec<&str> = told.iter().map(|(name, _, _)| name.as_str()).collect();
        ssh(&outbox.login, &format!("d={dir}; mkdir -p \"$d/{0}\" && cd \"$d\" && mv -f -- {1} {0}/ && rm -f -- {2}", queue::WITHDRAWN, names.join(" "), bundles(&names)))?;
        for (_, ticket, reason) in told {
            if let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) {
                super::after::send(session, &format!("north land: {} ({}) was withdrawn from {host}'s outbox: {reason}; queue it again from a worktree on its branch", ticket.branch, ticket.head));
            }
        }
        Ok(())
    }

    pub fn pull_remote(&self) -> Pulled {
        let mut pulled = Pulled::default();
        for host in self.remote_hosts() {
            if !plain(&host) {
                pulled.waiting.push(format!("the outbox hosts name {host:?}, which is not an ssh host name; skipped"));
                continue;
            }
            let outboxes = match route::outboxes(&host) {
                Ok(outboxes) => outboxes,
                Err(error) => {
                    pulled.waiting.push(format!("{host}'s outbox skipped this round: {error}"));
                    continue;
                }
            };
            for outbox in outboxes {
                self.pull_once(&outbox, &mut pulled);
            }
        }
        pulled
    }

    fn pull_once(&self, outbox: &Outbox, pulled: &mut Pulled) {
        let named = outbox.name();
        let mut taken = Vec::new();
        match self.pull_host(outbox, Duration::ZERO, &mut taken) {
            Ok(0) => {}
            Ok(count) => println!("north land: pulled {count} tickets from {named}'s outbox"),
            Err(error) => pulled.waiting.push(format!("{named}'s outbox skipped this round: {error}")),
        }
        if !taken.is_empty() {
            pulled.taken.push((outbox.clone(), taken));
        }
    }

    pub fn settle(&self, taken: &[(Outbox, Vec<String>)]) -> Vec<String> {
        let mut waiting = Vec::new();
        for (outbox, names) in taken {
            if let Err(error) = self.remote_outbox().and_then(|dir| ssh(&outbox.login, &format!("d={dir}; {}", settle_script(names)))) {
                waiting.push(format!("{}'s outbox keeps its taken tickets this round: {error}", outbox.name()));
            }
        }
        waiting
    }
}

#[derive(Default)]
pub struct Pulled {
    pub waiting: Vec<String>,
    pub taken: Vec<(Outbox, Vec<String>)>,
}

fn settle_script(taken: &[String]) -> String {
    if taken.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = taken.iter().map(String::as_str).collect();
    format!("(mkdir -p \"$d/taken\" && cd \"$d\" && mv -f -- {} taken/ && rm -f -- {}) || exit 1; ", names.join(" "), bundles(&names))
}

fn bundles(names: &[&str]) -> String {
    names.iter().map(|name| queue::bundle(Path::new(name)).to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")
}

fn ledger_tail() -> String {
    let rows = std::fs::read_to_string(super::post_land::ledger_path()).unwrap_or_default();
    let lines: Vec<&str> = rows.lines().collect();
    lines[lines.len().saturating_sub(500)..].iter().map(|line| format!("{line}\n")).collect()
}

fn astray(outbox: &Outbox, ticket: &Ticket) -> Option<String> {
    let host = &outbox.host;
    if ticket.host.split('.').next() != host.split('.').next() {
        return Some(format!("it names the host {:?}, not {host}, whose outbox it came from", ticket.host));
    }
    (route::lane_login(host, &ticket.root).ok().as_ref() != Some(&outbox.login)).then(|| format!("its worktree {} on {host} does not belong to the user whose outbox it came from", ticket.root.display()))
}

fn lane_gone(outbox: &Outbox, ticket: &Ticket) -> bool {
    let root = ticket.root.to_string_lossy().replace('\'', "'\\''");
    ssh(&outbox.login, &format!("[ -e '{root}/.git' ] && echo present || echo gone")).is_ok_and(|answer| answer.trim() == "gone")
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
