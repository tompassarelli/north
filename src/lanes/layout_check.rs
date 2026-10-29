use std::fs;
use std::path::{Path, PathBuf};

use crate::platform::git;

pub(super) const MINIMUM_REPOSITORIES: usize = 5;

const GUIDE: &str = "
The layout is:
  ~/code/<project>/                  container only — never a checkout
  ~/code/<project>/main/             the clean checkout — read-only product
  ~/code/<project>/worktrees/<slug>/ ephemeral lanes — sweepable
  ~/code/<project>/pins/<full-object-id>/    immutable detached checkout
  ~/code/<project>/pins/<full-object-id>.pin nonempty consumer metadata sidecar

Move a lane with:
  git -C ~/code/<project>/main worktree move <old> ~/code/<project>/worktrees/<slug>

If it is on a different filesystem (rename(2) cannot cross devices, which is
why /tmp worktrees fail), copy and repair instead:
  cp -a <old> ~/code/<project>/worktrees/<slug>
  git -C ~/code/<project>/main worktree repair ~/code/<project>/worktrees/<slug>
  git -C ~/code/<project>/main worktree prune

Create a pin at its immutable content address:
  PIN_OBJECT_ID=FULL_GIT_OBJECT_ID
  git -C ~/code/<project>/main worktree add --detach     ~/code/<project>/pins/$PIN_OBJECT_ID $PIN_OBJECT_ID
  # write ~/code/<project>/pins/$PIN_OBJECT_ID.pin, then update the consumer";

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) lines: Vec<String>,
    pub(super) repositories: usize,
    pub(super) checked: usize,
    pub(super) pins: usize,
    pub(super) strays: usize,
}

pub(crate) fn run(_arguments: &[String]) -> u8 {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let code = std::env::var_os("CODE_ROOT")
        .filter(|value| !value.is_empty())
        .map_or_else(|| home.join("code"), PathBuf::from);
    conclude(&scan(&code, &home), &code)
}

pub(super) fn conclude(report: &Report, code: &Path) -> u8 {
    for line in &report.lines {
        println!("{line}");
    }
    if report.repositories < MINIMUM_REPOSITORIES {
        eprintln!(
            "worktree-layout: FAILED — only {} repositories discovered under {};\n  discovery is broken, not the tree. Refusing to report a clean result.",
            report.repositories,
            code.display()
        );
        return 1;
    }
    if report.strays == 0 {
        println!(
            "worktree-layout: OK — {} trees, all at ~/<project>/main, /worktrees/<slug>, or /pins/<full-object-id> ({} pin(s))",
            report.checked, report.pins
        );
        return 0;
    }
    eprintln!("\nworktree-layout: {} worktree(s) out of format.\n{GUIDE}", report.strays);
    1
}

pub(super) fn scan(code: &Path, home: &Path) -> Report {
    let mut report = Report::default();
    let mut containers = fs::read_dir(code)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    containers.sort();
    for container in containers {
        let checkout = container.join("main");
        if !checkout.join(".git").is_dir() {
            continue;
        }
        report.repositories += 1;
        let repository = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for worktree in worktrees(&checkout) {
            report.checked += 1;
            classify(&mut report, code, home, &container, &repository, &checkout, &worktree);
        }
    }
    report
}

fn worktrees(checkout: &Path) -> Vec<PathBuf> {
    git::run(checkout, &["worktree", "list", "--porcelain"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn classify(
    report: &mut Report,
    code: &Path,
    home: &Path,
    container: &Path,
    repository: &str,
    checkout: &Path,
    worktree: &Path,
) {
    if worktree == checkout || worktree.strip_prefix(container.join("worktrees")).is_ok_and(|rest| !rest.as_os_str().is_empty()) {
        return;
    }
    let shown = display(worktree, home);
    if let Ok(leaf) = worktree.strip_prefix(container.join("pins")) {
        let leaf = leaf.to_string_lossy();
        if !leaf.is_empty() {
            match pin_problem(container, worktree, &leaf) {
                Some(problem) => {
                    report.lines.push(format!("INVALID {repository:<14} {shown} — {problem}"));
                    report.strays += 1;
                }
                None => {
                    report.lines.push(format!("PIN    {repository:<14} {shown}"));
                    report.pins += 1;
                }
            }
            return;
        }
    }
    if exempt(code, home, worktree) {
        return;
    }
    report.lines.push(format!("STRAY  {repository:<14} {shown}"));
    report.strays += 1;
}

fn exempt(code: &Path, home: &Path, worktree: &Path) -> bool {
    let text = worktree.to_string_lossy();
    let under = |marker: &str| text.split_once(marker).is_some_and(|(_, rest)| !rest.is_empty());
    under("/store-runtime/deployments/")
        || under("/experiments/")
        || worktree.strip_prefix(code.join("resources")).is_ok_and(|rest| !rest.as_os_str().is_empty())
        || [home.join(".cache"), home.join(".local/state")].iter().any(|owned| worktree.strip_prefix(owned).is_ok_and(|rest| !rest.as_os_str().is_empty()))
}

fn pin_problem(container: &Path, worktree: &Path, leaf: &str) -> Option<String> {
    let nested = leaf.contains('/').then(|| "pin must be directly under pins/".to_string());
    let head = git::run(worktree, &["rev-parse", "HEAD"]).ok().filter(|output| output.status.success());
    let Some(head) = head else {
        return Some("checkout HEAD is unreadable".into());
    };
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if head != leaf {
        return Some(format!("leaf is not the checkout's full object ID (HEAD is {head})"));
    }
    let attached = git::run(worktree, &["symbolic-ref", "-q", "HEAD"]).is_ok_and(|output| output.status.success());
    if attached {
        return Some("HEAD is attached to a branch".into());
    }
    let sidecar = container.join("pins").join(format!("{leaf}.pin"));
    if !sidecar.is_file() {
        return Some("same-name consumer sidecar is missing".into());
    }
    if fs::read(&sidecar).map_or(true, |text| text.iter().all(u8::is_ascii_whitespace)) {
        return Some("same-name consumer sidecar is empty".into());
    }
    nested
}

fn display(path: &Path, home: &Path) -> String {
    let text = path.to_string_lossy();
    let home = home.to_string_lossy();
    match text.strip_prefix(home.as_ref()) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => text.into_owned(),
    }
}
