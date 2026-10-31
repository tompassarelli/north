use super::*;
use std::fs;
use std::path::Path;

fn script(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

fn barrier(dir: &Path, name: &str, parties: usize) -> String {
    script(
        dir,
        name,
        &format!(
            "touch {dir}/{name}.ready\nfor _ in $(seq 1 200); do [ \"$(ls {dir}/*.ready | wc -l)\" -ge {parties} ] && {{ echo {name} met; exit 0; }}; sleep 0.05; done\necho {name} alone; exit 1\n",
            dir = dir.display()
        ),
    )
}

#[test]
fn selected_tests_run_side_by_side_up_to_the_job_limit() {
    let dir = tempfile::tempdir().unwrap();
    let tests: Vec<String> = (0..4).map(|i| barrier(dir.path(), &format!("t{i}.test.sh"), 4)).collect();
    let mut out = Vec::new();
    assert_eq!(run_all(&tests, NonZeroUsize::new(4).unwrap(), &mut out), Vec::<String>::new());
    let out = String::from_utf8(out).unwrap();
    for test in &tests {
        assert_eq!(out.lines().filter(|line| *line == format!("land check: {test}")).count(), 1);
    }
    assert_eq!(out.matches(" met").count(), 4, "{out}");
}

#[test]
fn a_red_test_fails_the_run_while_every_other_test_still_runs_and_reports_its_own_output() {
    let dir = tempfile::tempdir().unwrap();
    let red = script(dir.path(), "red.test.sh", "echo red says why\nexit 3\n");
    let green = script(dir.path(), "green.test.py", "print('green ran under python')\n");
    let later = script(dir.path(), "later.test.sh", "printf 'later ran'\n");
    let tests = vec![red.clone(), green.clone(), later.clone()];
    let mut out = Vec::new();
    assert_eq!(run_all(&tests, NonZeroUsize::MIN, &mut out), vec![red.clone()]);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("land check: {red}\nred says why\nland check: {red} exited 3 after ")), "{out}");
    assert!(out.contains(&format!("land check: {green}\ngreen ran under python\nland check: {green} passed")), "{out}");
    assert!(out.contains(&format!("land check: {later}\nlater ran\nland check: {later} passed")), "{out}");
}

#[test]
fn a_built_test_executable_without_an_extension_runs_as_itself() {
    let dir = tempfile::tempdir().unwrap();
    let built = script(dir.path(), "units-0123abcd", "#!/bin/sh\necho ran as $0\nexit 4\n");
    fs::set_permissions(&built, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut out = Vec::new();
    assert_eq!(run_all(std::slice::from_ref(&built), NonZeroUsize::MIN, &mut out), vec![built.clone()]);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("land check: {built}\nran as {built}\nland check: {built} exited 4 after ")), "{out}");
}
