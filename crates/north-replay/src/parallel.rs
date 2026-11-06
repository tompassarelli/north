//! Replay splits a kind's lines over the cores, since a day of admission calls is ten thousand
//! rebuilds of the facts and the pre-push scan has five seconds.
use std::thread;

/// Maps `work` over contiguous chunks of `items`, one thread each, and returns the results in chunk order.
pub fn chunks<T: Sync, R: Send>(items: &[T], work: impl Fn(&[T]) -> R + Sync) -> Vec<R> {
    let threads = thread::available_parallelism().map_or(1, usize::from).min(16);
    let size = items.len().div_ceil(threads).max(1);
    thread::scope(|scope| {
        let handles: Vec<_> = items.chunks(size).map(|part| scope.spawn(|| work(part))).collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
    })
}
