//! What the fleet screen reads in the background, each from the verb that owns the reading.
use crate::tui::fleet::Reads;

pub(super) fn reads() -> Reads {
    Reads { board: crate::cli::dash::for_fleet, rooms: super::rooms::of, scorecard: super::scorecard::of }
}
