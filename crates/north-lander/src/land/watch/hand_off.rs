use std::time::{Duration, Instant};

use super::journal::{Entry, journal, take_journaled};
use super::{Batch, LandedLane, Lander, Watch, epoch};

const LOOK: Duration = Duration::from_secs(30);

pub fn hand_off(lander: &Lander, base: &str, head: &str, lanes: Vec<LandedLane>) -> Result<(), String> {
    journal(lander, &Entry { base: base.to_owned(), head: head.to_owned(), lanes })
}

impl Watch {
    pub(super) fn take_handed_off(&mut self, lander: &Lander) -> bool {
        if self.handed_off_looked.is_some_and(|at| at.elapsed() < LOOK) {
            return false;
        }
        self.handed_off_looked = Some(Instant::now());
        self.queue.clone_from(&lander.queue);
        let mut taken = false;
        for (_, Entry { base, head, lanes }) in take_journaled(lander).unwrap_or_default() {
            if !self.follows(&head) {
                println!("north land: following the after-landing run of {head}, which landed past the lander");
                self.supersede(lander, &head);
                self.pending.add(Batch::landed(base, head, lanes, epoch()));
                taken = true;
            }
        }
        if taken {
            self.start_next(lander);
        }
        taken
    }
}
