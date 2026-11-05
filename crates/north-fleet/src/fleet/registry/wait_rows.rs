use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{NorthError, NorthResult};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind, Spool};
use crate::fleet::waits::Run;

use super::{Fleet, HandedIn};

#[derive(Debug, Eq, PartialEq)]
pub enum Waiting {
    Here,
    Spooled { id: String, host: String },
}

impl Fleet {
    pub fn register_wait(&self, session: &str, run: &Run, cwd: &Path) -> NorthResult<Waiting> {
        if self.session_dir(session)?.exists() {
            self.waits().register(session, run.clone(), cwd, None)?;
            return Ok(Waiting::Here);
        }
        let Some(held) = self.held_elsewhere(session)? else { return Err(NorthError::Usage(self.unknown_session(session))) };
        if matches!(run, Run::Capture(_) | Run::Process { .. }) {
            return Err(NorthError::Usage(format!("{run} names a path or process on this machine, which {}, where {session} runs, cannot see; wait on a job, farm or branch run instead", held.host)));
        }
        let row = self.new_row(RowKind::Wait, session, &run.to_string(), &Sender::typed(self.caller()))?;
        Spool::at(&self.root).put(&row)?;
        Ok(Waiting::Spooled { id: row.id, host: held.host })
    }

    pub(super) fn register_wait_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let run: Run = row.text.parse()?;
        self.journaled(row, || {
            let cwd = self.record(&row.to)?.cwd;
            self.waits().register(&row.to, run, &cwd, None)
        })
    }

    fn unknown_session(&self, name: &str) -> String {
        let elsewhere = self.cached_sessions().ok().flatten().unwrap_or_default().into_iter().map(|session| session.host).filter(|host| *host != self.host);
        let hosts: BTreeSet<String> = std::iter::once(self.host.clone()).chain(elsewhere).collect();
        format!("no session named {name} on any host checked ({}); the fleet table lists only live sessions", hosts.into_iter().collect::<Vec<_>>().join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::store::{SessionRow, cache_sessions};

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "laptop".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
    }

    struct Landed;

    impl crate::fleet::waits::Probe for Landed {
        fn result(&self, _: &crate::fleet::waits::Registered) -> Option<String> {
            Some("landed abc".into())
        }
    }

    fn run() -> Run {
        "branch:experiments-registry".parse().unwrap()
    }

    #[test]
    fn a_wait_for_a_session_another_host_holds_is_spooled_and_registered_by_that_host_exactly_once() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let _worker = live(&laptop, "worker");
        cached(&far, &["worker"]);

        let waiting = far.register_wait("worker", &run(), Path::new("/gex45/north")).unwrap();
        let Waiting::Spooled { host, .. } = waiting else { panic!("a session of another host is spooled") };
        let mut pending = Spool::at(&far.root).pending().unwrap();
        assert_eq!((host.as_str(), pending.len()), ("laptop", 1));
        assert!(far.waits().of("worker").is_empty());
        let row = pending.remove(0);
        assert_eq!((row.kind, row.to.as_str(), row.text.as_str()), (RowKind::Wait, "worker", "branch:experiments-registry"));

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);
        let kept = laptop.waits().of("worker");
        assert_eq!((kept.len(), kept[0].run.clone(), kept[0].cwd.clone()), (1, run(), "/tmp".into()), "the host probes from its own record of the session");

        let told = std::cell::RefCell::new(Vec::new());
        let settled = laptop.waits().settle(&Landed, &|name| laptop.session(name).is_ok(), &|name, text| {
            told.borrow_mut().push((name.to_owned(), text.to_owned()));
            Ok(())
        });
        assert_eq!(settled.iter().map(|settled| settled.line()).collect::<Vec<_>>(), ["worker: branch:experiments-registry landed abc"]);
        assert!(told.borrow()[0].1.contains("branch:experiments-registry: landed abc") && laptop.waits().of("worker").is_empty());
    }

    #[test]
    fn a_wait_for_a_session_this_host_holds_is_registered_at_once() {
        let home = tempfile::tempdir().unwrap();
        let here = fleet(home.path());
        let _worker = live(&here, "worker");

        assert_eq!(here.register_wait("worker", &run(), Path::new("/repo")).unwrap(), Waiting::Here);

        assert_eq!(here.waits().of("worker").len(), 1);
        assert!(Spool::at(&here.root).pending().unwrap().is_empty());
    }

    #[test]
    fn a_wait_for_a_name_no_host_holds_is_refused_naming_the_hosts_checked() {
        let home = tempfile::tempdir().unwrap();
        let far = fleet(home.path());
        cached(&far, &["worker"]);

        let Err(NorthError::Usage(why)) = far.register_wait("nobody", &run(), Path::new("/repo")) else { panic!("an unknown name is refused") };

        assert!(why.contains("nobody") && why.contains("laptop") && why.contains(&far.host), "{why}");
        assert!(Spool::at(&far.root).pending().unwrap().is_empty());
    }

    #[test]
    fn a_path_or_process_wait_cannot_cross_hosts() {
        let home = tempfile::tempdir().unwrap();
        let far = fleet(home.path());
        cached(&far, &["worker"]);

        for text in ["capture:/tmp/done", "pid:4242@99"] {
            assert!(matches!(far.register_wait("worker", &text.parse().unwrap(), Path::new("/repo")), Err(NorthError::Usage(why)) if why.contains("laptop")));
        }
        assert!(Spool::at(&far.root).pending().unwrap().is_empty());
    }
}
