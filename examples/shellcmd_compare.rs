#[path = "../src/shell/mod.rs"]
mod shell;
#[path = "../src/shell/tests.rs"]
mod comparison;

fn main() {
    comparison::compare_reference();
}
