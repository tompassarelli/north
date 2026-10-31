use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates;
use crate::fleet::relay::{self, Courier};
use crate::fleet::remote_view::{GateView, Seen, SessionView, Snapshot, split_qualified};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{SessionRow, cached_sessions};

use super::Fleet;

const REMOTE: &str = ".remote";
const VIEW: &str = "json";

impl Fleet {
    pub fn cached_sessions(&self) -> NorthResult<Option<Vec<SessionRow>>> {
        cached_sessions(&self.root)
    }

    pub fn snapshot(&self) -> NorthResult<Snapshot> {
        let sessions = self.sessions()?;
        let views = sessions.iter().map(|record| SessionView {
            name: record.name.clone(),
            provider: record.provider.label().to_owned(),
            account: record.account.clone(),
            status: self.status(record).label().to_owned(),
            parent: record.parent.clone(),
            cwd: match &record.machine {
                Some(machine) => format!("{}:{}", machine.name, record.cwd.display()),
                None => record.cwd.display().to_string(),
            },
            placement: record.placement.clone(),
            machine: record.machine.as_ref().map(|machine| machine.name.clone()),
            tasks: record.tasks().map(str::to_owned).collect(),
        });
        let open = gates::decisions(self.ledger.gates(0)?).into_iter().filter(|gate| gate.is_open());
        let gates = open.map(|gate| GateView { id: gate.id, session: gate.request.session, question: gate.request.question, options: gate.request.options, issue: gate.request.issue });
        Ok(Snapshot { host: relay::hostname(), sessions: views.collect(), gates: gates.collect() })
    }

    pub(super) fn remember(&self, ssh: &str, snapshot: &Snapshot) -> NorthResult<()> {
        let dir = self.root.join(REMOTE);
        fs::create_dir_all(&dir)?;
        let name: String = ssh.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
        let staged = dir.join(format!(".{name}.tmp"));
        fs::write(&staged, serde_json::to_vec(&Seen { ssh: ssh.to_owned(), at_ms: now_ms(), snapshot: snapshot.clone() })?)?;
        fs::rename(staged, view_path(&dir, &name))?;
        Ok(())
    }

    pub fn remote_views(&self) -> Vec<Seen> {
        let mut seen: Vec<Seen> = fs::read_dir(self.root.join(REMOTE))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == VIEW))
            .filter_map(|path| serde_json::from_slice(&fs::read(path).ok()?).ok())
            .collect();
        seen.sort_by(|a, b| a.ssh.cmp(&b.ssh));
        seen
    }

    pub fn answer_remote_gate(&self, courier: &dyn Courier, label: &str, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<String> {
        let (host, id) = split_qualified(label).ok_or_else(|| NorthError::Usage(format!("not a host-qualified gate id: {label}")))?;
        let views = self.remote_views();
        let seen = views.iter().find(|seen| seen.snapshot.host == host).ok_or_else(|| {
            let known: Vec<&str> = views.iter().map(|seen| seen.snapshot.host.as_str()).collect();
            NorthError::Usage(format!("no relayed host named {host}; relayed: {}", known.join(", ")))
        })?;
        courier.answer(&seen.ssh, caller, id, number, note)
    }
}

fn view_path(dir: &std::path::Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{VIEW}"))
}
