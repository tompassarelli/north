use std::sync::{Condvar, Mutex, PoisonError};
use std::thread;

use crate::error::{Result, RunError};
use crate::plan::Unit;

pub fn admits(running: usize, large_running: usize, unit_is_large: bool, in_flight: usize) -> bool {
    running < in_flight && !(unit_is_large && large_running > 0)
}

#[derive(Default)]
struct State {
    next: usize,
    running: usize,
    large_running: usize,
    failure: Option<RunError>,
}

pub fn execute<F>(units: &[Unit], in_flight: usize, exec: F) -> Result<()>
where
    F: Fn(&Unit) -> Result<()> + Sync,
{
    let state = Mutex::new(State::default());
    let changed = Condvar::new();
    thread::scope(|scope| {
        for _ in 0..in_flight {
            scope.spawn(|| {
                while let Some(index) = claim(&state, &changed, units, in_flight) {
                    let unit = &units[index];
                    let result = exec(unit);
                    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
                    guard.running -= 1;
                    if unit.is_large() {
                        guard.large_running -= 1;
                    }
                    if let Err(error) = result {
                        guard.failure.get_or_insert(error);
                    }
                    changed.notify_all();
                }
            });
        }
    });
    match state
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .failure
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn claim(
    state: &Mutex<State>,
    changed: &Condvar,
    units: &[Unit],
    in_flight: usize,
) -> Option<usize> {
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if guard.failure.is_some() || guard.next >= units.len() {
            return None;
        }
        let unit = &units[guard.next];
        if admits(
            guard.running,
            guard.large_running,
            unit.is_large(),
            in_flight,
        ) {
            let index = guard.next;
            guard.next += 1;
            guard.running += 1;
            if unit.is_large() {
                guard.large_running += 1;
            }
            return Some(index);
        }
        guard = changed.wait(guard).unwrap_or_else(PoisonError::into_inner);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::manifest;
    use crate::plan::Plan;

    #[test]
    fn a_large_task_never_starts_beside_another_large_task() {
        assert!(!admits(1, 1, true, 4));
        assert!(admits(1, 0, true, 4));
        assert!(!admits(2, 0, false, 2));
    }

    #[test]
    fn with_room_for_four_trials_no_two_large_tasks_overlap_and_every_unit_runs_once() {
        let tasks = manifest::all().expect("table parses");
        let units = Plan::full(3, &tasks).units;
        let running_large = AtomicUsize::new(0);
        let peak_large = AtomicUsize::new(0);
        let ran = AtomicUsize::new(0);
        execute(&units, 4, |unit| {
            if unit.is_large() {
                let now = running_large.fetch_add(1, Ordering::SeqCst) + 1;
                peak_large.fetch_max(now, Ordering::SeqCst);
                for _ in 0..200 {
                    thread::yield_now();
                }
                running_large.fetch_sub(1, Ordering::SeqCst);
            }
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("units run");
        assert_eq!(ran.load(Ordering::SeqCst), 89);
        assert_eq!(peak_large.load(Ordering::SeqCst), 1);
    }
}
