use super::{Lander, Watch};

pub(super) fn hand_over(lander: &Lander, watch: &Watch) -> u8 {
    if watch.journaled() > 0 {
        let handed = if watch.adoptable(lander) { "with every after-landing run in flight held by a runner the restarted lander adopts" } else { "with a culprit search or a runner-less after-landing step still in flight that the restarted lander reruns" };
        println!("north land: the live runtime changed; exiting at once {handed}; it resumes the rest of the {} journaled batch(es) as one run where they follow each other on main", watch.journaled());
    }
    0
}
