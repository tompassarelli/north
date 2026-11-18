use std::time::{Duration, Instant};

use crate::error::Result;

/// How long one graph write transaction may run before the next chunk halves: a guess from north#1654, where a 13.3 s retain transaction and the coverage ingest's single transaction tripped the seat breaker; the night's retain `longest` and the seat probe samples settle it.
const WRITE_BUDGET: Duration = Duration::from_millis(250);

/// Writes `items` in order, one transaction per chunk: the first chunk holds an eighth of `most`, a chunk that runs past the write budget halves the next, and one under a quarter of it doubles the next up to `most`.
pub(crate) fn paced<T, R>(items: &[T], most: usize, mut write: impl FnMut(&[T]) -> Result<R>) -> Result<Vec<R>> {
    let mut size = (most / 8).max(1);
    let mut rest = items;
    let mut written = Vec::new();
    while !rest.is_empty() {
        let (chunk, after) = rest.split_at(size.min(rest.len()));
        let started = Instant::now();
        written.push(write(chunk)?);
        let took = started.elapsed();
        size = if took > WRITE_BUDGET { (size / 2).max(1) } else if took < WRITE_BUDGET / 4 { size.saturating_mul(2).min(most) } else { size };
        rest = after;
    }
    Ok(written)
}
