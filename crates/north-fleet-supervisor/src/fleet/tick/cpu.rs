use std::time::Duration;

use rustix::time::{ClockId, clock_gettime};

use crate::fleet::stage::{Detached, detach};

#[derive(Clone, Copy)]
pub struct Cpu {
    clock: ClockId,
    own: Duration,
    children: Duration,
}

impl Cpu {
    pub fn thread() -> Self {
        Self::read(ClockId::ThreadCPUTime)
    }

    pub fn process() -> Self {
        Self::read(ClockId::ProcessCPUTime)
    }

    fn read(clock: ClockId) -> Self {
        let spent = clock_gettime(clock);
        let own = Duration::new(u64::try_from(spent.tv_sec).unwrap_or(0), u32::try_from(spent.tv_nsec).unwrap_or(0));
        Self { clock, own, children: children() }
    }

    pub fn spent(self) -> Duration {
        let now = Self::read(self.clock);
        now.own.saturating_sub(self.own) + now.children.saturating_sub(self.children)
    }
}

pub fn metered<R>(run: impl FnOnce() -> R) -> (R, Duration) {
    let cpu = Cpu::thread();
    let ran = run();
    (ran, cpu.spent())
}

pub fn detach_metered<R: Send + 'static>(name: &str, run: impl FnOnce() -> R + Send + 'static) -> Detached<(R, Duration)> {
    detach(name, move || metered(run))
}

fn children() -> Duration {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else { return Duration::ZERO };
    let Some((_, fields)) = stat.rsplit_once(')') else { return Duration::ZERO };
    let ticks: u64 = fields.split_whitespace().skip(13).take(2).filter_map(|field| field.parse::<u64>().ok()).sum();
    // proc(5) reports cutime and cstime in USER_HZ ticks, which Linux fixes at 100 per second.
    Duration::from_millis(ticks * 10)
}
