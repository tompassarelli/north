use std::fs;
use std::path::Path;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

use crate::error::Result;
use crate::manifest::{self, Task};
use crate::proc;

const SETTLE: Duration = Duration::from_secs(5);
const TRIAL_DIRECTORIES: [&str; 4] = ["trials", "arms", "jobs", "ws"];
const LIVE_UNITS: &str = "--state=active,activating,deactivating,reloading";
const CONTAINER: &str = "{{.ImageName}} {{.State.Status}}{{range .Mounts}} {{.Source}}{{end}}";

pub fn stop(work: &Path) -> Result<bool> {
    let tasks = manifest::all()?;
    let places = places(work);
    signal_runners(work)?;
    stop_units()?;
    signal_processes(&places, "-TERM")?;
    remove_containers(&tasks, &places)?;
    thread::sleep(SETTLE);
    signal_processes(&places, "-KILL")?;
    remove_containers(&tasks, &places)?;
    thread::sleep(Duration::from_secs(1));
    let remains = [
        report("containers", containers(&tasks, &places)),
        report("units", units()),
        report("processes", Some(processes(&places))),
    ];
    Ok(remains.iter().any(|remaining| *remaining))
}

fn places(work: &Path) -> Vec<String> {
    let mut roots = vec![work.to_path_buf()];
    let real = fs::canonicalize(work).unwrap_or_else(|_| work.to_path_buf());
    if real != work {
        roots.push(real);
    }
    roots
        .iter()
        .flat_map(|root| TRIAL_DIRECTORIES.map(|name| root.join(name).display().to_string()))
        .collect()
}

fn report(what: &str, lines: Option<Vec<String>>) -> bool {
    let Some(lines) = lines else {
        println!("{what}: could not be listed");
        return true;
    };
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
    let Ok(entries) = fs::read_dir(work.join("runs")) else {
        return Ok(());
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(recorded) = fs::read_to_string(entry.path().join("runner.pid")) else {
            continue;
        };
        let pid = recorded.trim();
        let command = text_of(&Path::new("/proc").join(pid).join("cmdline"));
        if command.contains("eval-x-runner") && pid.parse::<u32>().ok() != Some(process::id()) {
            kill(pid, "-TERM")?;
        }
    }
    Ok(())
}

fn stop_units() -> Result<()> {
    for unit in units().unwrap_or_default() {
        if let Some(name) = unit.split_whitespace().next() {
            proc::status(Command::new("systemctl").args(["--user", "stop", name]))?;
        }
    }
    Ok(())
}

fn units() -> Option<Vec<String>> {
    let own = own_unit();
    let listed = capture(Command::new("systemctl").args([
        "--user",
        "list-units",
        "--plain",
        "--no-legend",
        LIVE_UNITS,
        "eval-x-*",
    ]))?;
    Some(
        listed
            .lines()
            .filter(|line| line.split_whitespace().next() != own.as_deref())
            .map(str::to_owned)
            .collect(),
    )
}

fn own_unit() -> Option<String> {
    let cgroup = fs::read_to_string("/proc/self/cgroup").ok()?;
    let unit = cgroup.trim().rsplit('/').next()?;
    unit.starts_with("eval-x-").then(|| unit.to_owned())
}

fn containers(tasks: &[Task], places: &[String]) -> Option<Vec<String>> {
    let listed = capture(proc::podman().args(["ps", "-a", "-q"]))?;
    let mut found = Vec::new();
    for id in listed.lines() {
        let mut inspect = proc::podman();
        inspect.args(["inspect", "--format", CONTAINER, id]);
        let Some(described) = capture(&mut inspect) else {
            continue;
        };
        let described = described.trim();
        let of_a_task = tasks.iter().any(|task| described.contains(&task.image));
        if of_a_task || places.iter().any(|place| described.contains(place)) {
            found.push(format!("{id} {described}"));
        }
    }
    Some(found)
}

fn remove_containers(tasks: &[Task], places: &[String]) -> Result<()> {
    for line in containers(tasks, places).unwrap_or_default() {
        if let Some(id) = line.split_whitespace().next() {
            proc::status(proc::podman().args(["rm", "-f", "-t", "0", id]))?;
        }
    }
    Ok(())
}

fn processes(places: &[String]) -> Vec<String> {
    let spared = lineage();
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        if spared.contains(&pid) {
            continue;
        }
        let directory = entry.path();
        let command = text_of(&directory.join("cmdline"));
        let environment = text_of(&directory.join("environ"));
        let cwd = fs::read_link(directory.join("cwd"))
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let names_a_trial = places.iter().any(|place| {
            command.contains(place) || environment.contains(place) || cwd.starts_with(place)
        });
        if names_a_trial {
            found.push(format!("{pid} {}", command.trim()));
        }
    }
    found
}

fn lineage() -> Vec<u32> {
    let mut line = vec![process::id()];
    while let Some(parent) = line.last().and_then(|pid| parent_of(*pid)) {
        if parent <= 1 {
            break;
        }
        line.push(parent);
    }
    line
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(')')?.1;
    after_name.split_whitespace().nth(1)?.parse().ok()
}

fn signal_processes(places: &[String], signal: &str) -> Result<()> {
    for line in processes(places) {
        if let Some(pid) = line.split_whitespace().next() {
            kill(pid, signal)?;
        }
    }
    Ok(())
}

fn kill(pid: &str, signal: &str) -> Result<()> {
    proc::status(Command::new("kill").args([signal, pid]))?;
    Ok(())
}

fn text_of(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
        .unwrap_or_default()
}

fn capture(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
