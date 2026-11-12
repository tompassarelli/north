use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::error::Result;
use crate::manifest;

const SETTLE: Duration = Duration::from_secs(5);

pub fn stop(work: &Path) -> Result<bool> {
    let images = manifest::all()?;
    signal_runners(work)?;
    stop_units()?;
    remove_containers(&images)?;
    kill_processes(work)?;
    thread::sleep(SETTLE);
    let remains = [
        report("containers", &containers(&images)?),
        report("units", &units()?),
        report("processes", &processes(work)?),
    ];
    Ok(remains.iter().any(|remaining| *remaining))
}

fn report(what: &str, lines: &[String]) -> bool {
    if lines.is_empty() {
        println!("{what}: none");
        return false;
    }
    println!("{what} remaining:");
    for line in lines {
        println!("  {line}");
    }
    true
}

fn signal_runners(work: &Path) -> Result<()> {
    let runs = work.join("runs");
    let Ok(entries) = fs::read_dir(&runs) else {
        return Ok(());
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        if let Ok(pid) = fs::read_to_string(entry.path().join("runner.pid")) {
            kill(pid.trim(), "-TERM")?;
        }
    }
    Ok(())
}

fn stop_units() -> Result<()> {
    for unit in units()? {
        if let Some(name) = unit.split_whitespace().next() {
            Command::new("systemctl")
                .args(["--user", "stop", name])
                .status()?;
        }
    }
    Ok(())
}

fn units() -> Result<Vec<String>> {
    Ok(capture(
        "systemctl",
        &[
            "--user",
            "list-units",
            "--all",
            "--plain",
            "--no-legend",
            "eval-x-*",
        ],
    )
    .lines()
    .map(str::to_owned)
    .collect())
}

fn containers(images: &[manifest::Task]) -> Result<Vec<String>> {
    Ok(capture(
        "podman",
        &["ps", "-a", "--format", "{{.ID}} {{.Image}} {{.Status}}"],
    )
    .lines()
    .filter(|line| images.iter().any(|task| line.contains(&task.image)))
    .map(str::to_owned)
    .collect())
}

fn remove_containers(images: &[manifest::Task]) -> Result<()> {
    for line in containers(images)? {
        if let Some(id) = line.split_whitespace().next() {
            Command::new("podman").args(["rm", "-f", id]).status()?;
        }
    }
    Ok(())
}

fn processes(work: &Path) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for pattern in [work.join("trials"), work.join("arms"), work.join("jobs")] {
        let text = pattern.display().to_string();
        lines.extend(
            capture("pgrep", &["-af", "--", &text])
                .lines()
                .map(str::to_owned),
        );
    }
    Ok(lines)
}

fn kill_processes(work: &Path) -> Result<()> {
    for line in processes(work)? {
        if let Some(pid) = line.split_whitespace().next() {
            kill(pid, "-TERM")?;
        }
    }
    Ok(())
}

fn kill(pid: &str, signal: &str) -> Result<()> {
    Command::new("kill").args([signal, pid]).status()?;
    Ok(())
}

fn capture(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}
