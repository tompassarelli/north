use std::path::Path;
use std::process::Command;

const GOAL_LINES: &str = "import sys, tomllib\nsys.path.insert(0, sys.argv[1])\nimport goal_eta\n\
with open(sys.argv[2], 'rb') as f:\n    cfg = tomllib.load(f)\nprint('\\n'.join(goal_eta.plan_lines(cfg, sys.argv[3])))";

pub(super) fn lines(harness: &Path, config: &Path, ledger: &Path) -> Vec<String> {
    let output = Command::new("python3")
        .args(["-I", "-c", GOAL_LINES])
        .arg(harness.join("harness/lib"))
        .arg(config)
        .arg(ledger)
        .output();
    match output {
        Ok(done) if done.status.success() => {
            eprint!("{}", String::from_utf8_lossy(&done.stderr));
            String::from_utf8_lossy(&done.stdout).lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
        }
        Ok(done) => {
            eprint!("{}", String::from_utf8_lossy(&done.stderr));
            Vec::new()
        }
        Err(error) => {
            eprintln!("agents plan: cannot run goal ETA: {error}");
            Vec::new()
        }
    }
}
