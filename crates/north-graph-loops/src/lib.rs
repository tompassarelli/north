//! The learning loops' facts in north's graph: each tick a loop's writer tells its learned values and
//! its tick instant in one transaction, and every decision reads them back through its rule under
//! harness/graph-rules/. A cell `<measure> <family> <category>` is the triple
//! `(cell:<category>:<family> :cell/<measure> value)`, and a tick is `(loop:<writer> :loop/tick-at instant)`.
mod capacity_estimate;
mod cells;
mod check_deadline;
pub mod code_health;
pub mod corrections;
mod facts;
mod fleet;
mod lander_doctor;
mod rules;
pub mod scoreboard;
pub mod skill;
pub mod slow_test;

#[cfg(test)]
mod land_tests;
#[cfg(test)]
mod tests;

pub use capacity_estimate::{Estimate, MEASURES as CAPACITY_MEASURES, WRITER as CAPACITY_WRITER, estimate, estimate_from, tell_estimate, tell_estimate_to};
pub use cells::{Cells, Learned, Measure};
pub use check_deadline::{CheckDeadline, WRITER as CHECK_WRITER, check_deadline, check_deadline_from, check_key, tell_check_deadline, tell_check_deadline_to};
pub use fleet::{Retirement, WRITER as FLEET_WRITER, retirement, retirement_from, tell_retirement, tell_retirement_to};
pub use lander_doctor::{DOCTOR_WRITER, Held, Remembered, remembered, remembered_from, tell_remembered, tell_remembered_to};
pub use north_graph::default_path as graph_path;
pub use rules::{read, read_from, tell, tell_to, ticks, ticks_from};
