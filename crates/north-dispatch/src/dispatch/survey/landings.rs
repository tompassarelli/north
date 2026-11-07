use super::super::issues::Issue;

/// When the newest commit the issue's body cites landed in its repository's main checkout, as unix seconds; 0 when none reads.
pub(super) fn landed_at(issue: &Issue) -> f64 {
    let Some(home) = std::env::var_os("HOME") else { return 0.0 };
    let checkout = std::path::Path::new(&home).join("code").join(crate::threads::catalog::checkout(&issue.repo)).join("main");
    let mut args = vec!["-C", checkout.to_str().unwrap_or_default(), "log", "--no-walk", "--format=%ct"];
    args.extend(issue.cited_shas());
    let Some(output) = crate::platform::git::run_within(&args, super::PROBE).filter(|output| output.status.success()) else { return 0.0 };
    String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.trim().parse::<f64>().ok()).fold(0.0, f64::max)
}

/// The landings on origin/main of the issue's repository since the issue's last report whose message cites the issue; 0 when none reads.
pub(super) fn since(issue: &Issue) -> usize {
    let Some(home) = std::env::var_os("HOME") else { return 0 };
    let checkout = std::path::Path::new(&home).join("code").join(crate::threads::catalog::checkout(&issue.repo)).join("main");
    let since = format!("--since=@{}", issue.touched as i64);
    let args = ["-C", checkout.to_str().unwrap_or_default(), "log", "origin/main", &since, "--format=%B%x1e"];
    let Some(output) = crate::platform::git::run_within(&args, super::PROBE).filter(|output| output.status.success()) else { return 0 };
    let reference = issue.reference();
    String::from_utf8_lossy(&output.stdout).split('\x1e').filter(|message| super::super::issues::references(message, &issue.repo).contains(&reference)).count()
}
