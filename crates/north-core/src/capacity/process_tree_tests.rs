use super::render;
use std::fs;
use std::path::Path;

fn process(proc: &Path, pid: i32, parent: i32, group: i32, argv: &[&str]) {
    let directory = proc.join(pid.to_string());
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("stat"), format!("{pid} (x y) S {parent} {group} {group} 0 -1\n")).unwrap();
    fs::write(directory.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
}

#[test]
fn a_hung_group_renders_as_a_tree_of_its_command_lines() {
    let proc = tempfile::tempdir().unwrap();
    process(proc.path(), 10, 1, 10, &["bash", "-c", "land-checks.sh"]);
    process(proc.path(), 12, 10, 10, &["north", "land-tests", "a.test.sh"]);
    process(proc.path(), 15, 12, 10, &["bash", "a.test.sh"]);
    process(proc.path(), 16, 15, 16, &["sleep", "600"]);
    process(proc.path(), 20, 1, 20, &["unrelated"]);
    process(proc.path(), 30, 1, 10, &[]);
    assert_eq!(
        render(proc.path(), 10),
        "10 S bash -c land-checks.sh\n  12 S north land-tests a.test.sh\n    15 S bash a.test.sh\n      16 S sleep 600\n30 S [x y]\n"
    );
}
