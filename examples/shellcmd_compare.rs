#![allow(dead_code)] // path-included shell modules expose more than this example calls
#[path = "../crates/north-base/src/shell/mod.rs"]
mod shell;
#[path = "../crates/north-base/src/shell/tests.rs"]
mod comparison;

fn main() {
    comparison::compare_reference();
}
