//! `north coverage`: a map from each Rust test to the functions it executes, recorded on a check
//! host from an instrumented nextest run, and the pre-push test selection it drives: the tests that
//! reach the functions a lane changed, the tests of a whole changed file where the map is older than
//! that code, and none where the lane changes an input the map does not trace. The full suite still
//! runs after the push. Depends only on north-base; the lander reaches it through `north coverage`.
pub mod always;
pub mod exec;
pub mod hunks;
pub mod map;
pub mod miss;
pub mod pick;
pub mod record;
pub mod refresh;
pub mod run_selected;

const USAGE: &str = "usage: north coverage select BASE HEAD [--map FILE]\n       north coverage test BASE HEAD [--map FILE] [--timeout SECONDS]\n       north coverage miss BASE HEAD --log FILE [--map FILE]\n       north coverage record [NEXTEST-ARGUMENT...]\n       north coverage refresh [--repo NAME] [--host NAME] [--head REVISION]\n       north coverage exec BINARY [ARGUMENT...]";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "select" => run_selected::select(rest),
        Some((verb, rest)) if verb == "test" => run_selected::test(rest),
        Some((verb, rest)) if verb == "miss" => miss::run(rest),
        Some((verb, rest)) if verb == "record" => record::run(rest),
        Some((verb, rest)) if verb == "refresh" => refresh::run(rest),
        Some((verb, rest)) if verb == "exec" => exec::run(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
