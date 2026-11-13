//! `north test coverage`: a map from each Rust test to the functions it executes, recorded on a check
//! host from an instrumented nextest run with a `rust-analyzer scip` index of the functions that refer to each
//! item, and the selection it answers: the tests that reach the functions a lane changed, the tests of a whole
//! changed file where the map is older than that code, and none where the lane changes an input the map does not
//! trace. The change graph reads the map to pick the nextest check's tests, and the full suite still runs after the
//! push (north#944). Depends only on north-base; the lander reaches it through `north test coverage`.
pub mod cadence;
pub mod exec;
mod handoff;
pub mod hosts;
pub mod hunks;
pub mod index;
pub mod map;
pub mod miss;
pub mod pick;
pub mod reads;
pub mod record;
pub mod refresh;
mod refresh_start;
pub mod run_selected;
pub mod take;

const USAGE: &str = "usage: north test coverage select BASE HEAD [--map FILE]\n       north test coverage miss BASE HEAD --log FILE [--map FILE]\n       north test coverage record [NEXTEST-ARGUMENT...]\n       north test coverage refresh [--repo NAME] [--host NAME] [--head REVISION] | --start [--unknown N --listed M]\n       north test coverage take [--repo NAME]\n       north test coverage exec BINARY [ARGUMENT...]";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "select" => run_selected::select(rest),
        Some((verb, rest)) if verb == "miss" => miss::run(rest),
        Some((verb, rest)) if verb == "record" => record::run(rest),
        Some((verb, rest)) if verb == "refresh" && rest.first().is_some_and(|flag| flag == "--start") => refresh_start::run(&rest[1..]),
        Some((verb, rest)) if verb == "refresh" => refresh::run(rest),
        Some((verb, rest)) if verb == "exec" => exec::run(rest),
        Some((verb, rest)) if verb == "take" => take::run(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
