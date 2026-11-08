pub(super) fn graph(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "ready" => north_graph_ground::ready_command(rest),
        Some((verb, rest)) if verb == "ask" => north_graph_change::ask(rest),
        _ => north_graph_ingest::run(arguments),
    }
}
