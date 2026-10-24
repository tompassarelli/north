#![allow(dead_code)] // path-included shell modules expose more than this example calls
#[path = "../src/shell/mod.rs"]
mod shell;
#[path = "../src/shell/tests.rs"]
mod comparison;

fn main() {
    comparison::compare_reference();
}
