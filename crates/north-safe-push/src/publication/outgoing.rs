use std::path::Path;

use north_base::foundation::step::{Refused, Step};

use crate::session::identity::{Identity, state_changed};
use crate::shell::Shell;
use crate::shell::origin::Snapshot;

pub struct Outgoing {
    pub identity: Identity,
    pub destination_ref: String,
    pub snapshot: Snapshot,
    pub base: String,
    pub head: String,
}

impl Outgoing {
    pub fn capture(shell: &Shell, directory: &Path, to: &str) -> Step<Self> {
        let git = &*shell.git;
        let identity = Identity::capture(directory)?;
        let destination_ref = format!("refs/heads/{to}");
        let snapshot = remote_destination(shell, &identity.origin_url, &destination_ref)?;
        let head = identity.head_oid().to_owned();
        let base = match &snapshot {
            Snapshot::Object(oid) => {
                if !git.has_commit(oid) {
                    eprintln!("safe-push: origin {destination_ref} is not available locally at {oid} — fetch it first");
                    return Err(Refused(1));
                }
                if !git.is_ancestor(oid, &head) {
                    eprintln!("safe-push: non-fast-forward {head} -> {destination_ref} (origin at {oid}) — refusing; rebase onto origin {to} first");
                    return Err(Refused(1));
                }
                oid.clone()
            }
            Snapshot::Absent => shell.origin.head(&identity.origin_url).filter(|oid| git.has_commit(oid)).unwrap_or_default(),
        };
        Ok(Self { identity, destination_ref, snapshot, base, head })
    }

    pub fn revalidate(&self, shell: &Shell, directory: &Path) -> Step {
        self.identity.revalidate(directory)?;
        match shell.origin.snapshot(&self.identity.origin_url, &self.destination_ref) {
            Ok(current) if current == self.snapshot => Ok(()),
            Ok(_) => Err(state_changed(&format!("origin destination {} changed", self.destination_ref))),
            Err(refusal) => {
                eprint!("{refusal}");
                Err(state_changed("origin destination became unavailable, symbolic, malformed, or ambiguous"))
            }
        }
    }

    pub fn range(&self) -> String {
        if self.base.is_empty() { self.head.clone() } else { format!("{}..{}", self.base, self.head) }
    }

    pub fn snapshot_oid_or<'a>(&'a self, absent: &'a str) -> &'a str {
        match &self.snapshot {
            Snapshot::Object(oid) => oid,
            Snapshot::Absent => absent,
        }
    }
}

fn remote_destination(shell: &Shell, origin_url: &str, destination_ref: &str) -> Step<Snapshot> {
    shell.origin.snapshot(origin_url, destination_ref).map_err(|refusal| {
        eprint!("{refusal}");
        Refused(1)
    })
}
