use north_graph_change::split::Planner;

use super::graph::{admits, ask};

pub(super) fn work_split(arguments: &[String]) -> u8 {
    north_graph_change::split::task::run(arguments, &Planner { admits: &admits, ask: &ask }, &north_context_pack::pack)
}
