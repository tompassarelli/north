//! Which rung a box starts on, judged by the finished boxes most like it rather than by the category its
//! brief declares (north#705, docs/rationale/nearest-routing.md): a box's shape is read from what its text
//! names, each finished run's outcome is put down to the worker or the harness, and a rung fits a shape
//! when the worker-caused outcomes of its nearest finished boxes on that rung clear the success bar.
mod cause;
mod pick;
mod replay;
mod shape;

pub use replay::run as replay;
