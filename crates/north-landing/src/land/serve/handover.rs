use std::time::{Duration, Instant};

use super::{Lander, Step, TICK, Watch};

const HANDOVER: Duration = Duration::from_secs(900);

pub(super) fn hand_over(lander: &Lander, watch: &mut Watch, retiring: &mut Option<Instant>) -> Option<u8> {
    let since = *retiring.get_or_insert_with(|| {
        watch.drain();
        Instant::now()
    });
    if !watch.adoptable(lander) && since.elapsed() < HANDOVER {
        lander.heartbeat.enter(Step::Between);
        watch.settle(lander);
        std::thread::sleep(TICK);
        return None;
    }
    if watch.journaled() > 0 {
        let handed = if watch.adoptable(lander) { "with every after-landing run in flight held by a runner the restarted lander adopts" } else { "at the handover bound, with a culprit search or a runner-less after-landing step still in flight that the restarted lander reruns" };
        println!("north land: the live runtime changed; exiting after {} s {handed}; it resumes the rest of the {} journaled batch(es) as one run where they follow each other on main", since.elapsed().as_secs(), watch.journaled());
    }
    Some(0)
}
