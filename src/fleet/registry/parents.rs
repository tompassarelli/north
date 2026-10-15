use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::NorthResult;

use super::super::host::{Host, alive};
use super::super::model::{Delivery, SessionRecord};
use super::super::receipts::{Courier, HostDeath, deliver_death, still_watched};
use super::super::runtime::{Runtime, binary, now_ms};
use super::super::turns::Turns;
use super::{Fleet, recent, recent_frames};

const WATCHDOG_POLL: Duration = Duration::from_secs(1);

impl Fleet {
    pub fn guard(&self, name: &str) -> NorthResult<()> {
        let Some(host_pid) = self.record(name)?.host_pid else { return Ok(()) };
        while alive(host_pid) && still_watched(self.record(name).ok().as_ref(), host_pid) {
            std::thread::sleep(WATCHDOG_POLL);
        }
        let Ok(child) = self.record(name) else { return Ok(()) };
        let runtime = self.runtime(&child)?;
        let lines = recent(runtime.as_ref(), &child)?.entries;
        let death = HostDeath { child: &child, host_pid, mid_task: mid_task(runtime.as_ref(), &child)?, lines: &lines };
        if deliver_death(self.ledger.as_ref(), &Inline(self), &death, now_ms())? {
            Host::new(child.host_dir.clone()).terminate();
        }
        Ok(())
    }

    pub(super) fn tell(&self, parent: &str, text: &str, delivery: Delivery) {
        let told = self.record(parent).and_then(|record| if record.stopped { Ok(()) } else { self.send(parent, text, delivery) });
        if let Err(error) = told {
            eprintln!("north fleet: {parent} was not told: {}", error.user_message());
        }
    }

    pub(super) fn watch_over(&self, record: &SessionRecord) -> NorthResult<()> {
        if record.parent.is_none() {
            return Ok(());
        }
        let mut watchdog = Command::new(binary("setsid"))
            .arg(std::env::current_exe()?)
            .args(["fleet", "watchdog", &record.name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        std::thread::spawn(move || watchdog.wait());
        Ok(())
    }
}

impl Courier for Fleet {
    fn post(&self, parent: &str, text: String, delivery: Delivery) {
        self.tell(parent, &text, delivery);
    }
}

struct Inline<'a>(&'a Fleet);

impl Courier for Inline<'_> {
    fn post(&self, parent: &str, text: String, delivery: Delivery) {
        self.0.tell(parent, &text, delivery);
    }
}

fn mid_task(runtime: &dyn Runtime, record: &SessionRecord) -> NorthResult<bool> {
    let Some(conversation) = record.conversation.as_deref() else { return Ok(false) };
    let mut turns = Turns::default();
    for frame in recent_frames(record)? {
        for signal in runtime.signals(&frame) {
            turns.observe(signal, 0);
        }
    }
    Ok(turns.busy(conversation))
}
