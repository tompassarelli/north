#[path = "../src/shellcmd.rs"]
mod shellcmd;
#[path = "../src/shellcmd_tests.rs"]
mod comparison;

fn main() {
    comparison::compare_reference();
}
