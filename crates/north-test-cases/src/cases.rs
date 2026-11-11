use std::path::Path;
use std::process::Command;

pub struct Listed {
    pub run: Vec<String>,
    pub ignored: usize,
}

pub fn list(binary: &Path) -> Result<Listed, String> {
    let all = names(binary, &[])?;
    let ignored = names(binary, &["--ignored"])?;
    let run = all.into_iter().filter(|name| !ignored.contains(name)).collect();
    Ok(Listed { run, ignored: ignored.len() })
}

fn names(binary: &Path, filter: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new(binary)
        .args(["--list", "--format", "terse"])
        .args(filter)
        .output()
        .map_err(|error| format!("cannot list {}: {error}", binary.display()))?;
    if !output.status.success() {
        return Err(format!("{} --list exited {}: {}", binary.display(), output.status, String::from_utf8_lossy(&output.stderr)));
    }
    // libtest's terse list is one `NAME: test` or `NAME: benchmark` line per case.
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_suffix(": test")).map(str::to_owned).collect())
}
