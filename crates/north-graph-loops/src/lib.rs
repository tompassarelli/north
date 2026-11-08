//! The learning loops' facts in north's graph: each tick a loop's writer tells its learned values and
//! its tick instant in one transaction, and every decision reads them back through its rule under
//! harness/graph-rules/. A cell `<measure> <family> <category>` is the triple
//! `(cell:<category>:<family> :cell/<measure> value)`, and a tick is `(loop:<writer> :loop/tick-at instant)`.
mod cells;
mod rules;

#[cfg(test)]
mod tests;

pub use cells::{Cells, Learned, Measure};
pub use north_graph::default_path as graph_path;
pub use rules::{read, read_from, tell, tell_to, ticks, ticks_from};
