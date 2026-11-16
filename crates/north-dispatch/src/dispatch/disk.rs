use super::disk_stores;
use super::record;
use super::slots::Host;
use super::survey::Survey;
use north_base::platform::disk_read;
use std::path::Path;
use std::time::Duration;

const READS: u64 = 6;
const KEPT: &str = "disk-read.txt";
const KEPT_AT: &str = "disk-keep.txt";
const FRESH: Duration = Duration::from_secs(3600);
const CLOSED: &str = "free on the root disk";

/// The disk lines of this dispatch host's harness keep and of each host dispatch skipped for its root disk, under the build floor or its disk bound, after the run has decided and started its work. This host's keep runs at most once an hour whether or not this host is closed, so its runtimes never reach the bound; a dry run keeps nothing and reads afresh. A real run reuses the lines it kept for the closed hosts within the hour, so a host that stays closed costs one read an hour.
pub(super) fn read(survey: &Survey, dry_run: bool) -> Vec<String> {
    let kept = record::directory().join(KEPT);
    let kept_keep = keep(survey, dry_run);
    if let Some(text) = std::fs::read_to_string(&kept).ok().filter(|_| !dry_run && is_fresh(&kept)) {
        return kept_keep.into_iter().chain(text.lines().map(str::to_owned)).collect();
    }
    let lines = lines(survey);
    if !dry_run && !lines.is_empty() {
        let _ = std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::write(&kept, lines.join("\n")));
    }
    kept_keep.into_iter().chain(lines).collect()
}

/// This host's disk lines: those `read` kept when this host is closed for its disk, else a fresh read of its harness stores.
pub(super) fn here(survey: &Survey, kept: &[String]) -> Vec<String> {
    let prefix = format!("host {} disk: ", survey.here);
    let keep = format!("{prefix}keep:");
    let lines: Vec<String> = kept.iter().filter(|line| line.starts_with(&prefix) && !line.starts_with(&keep)).cloned().collect();
    if !lines.is_empty() {
        return lines;
    }
    survey.home.as_deref().map(disk_stores::read).unwrap_or_default().into_iter().map(|line| format!("{prefix}{}", line.trim_start())).collect()
}

fn is_fresh(path: &Path) -> bool {
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < FRESH)
}

fn keep(survey: &Survey, dry_run: bool) -> Vec<String> {
    let Some(home) = survey.home.as_deref() else { return Vec::new() };
    let stamp = record::directory().join(KEPT_AT);
    if dry_run || is_fresh(&stamp) {
        return Vec::new();
    }
    let _ = std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::write(&stamp, ""));
    disk_stores::keep(home).into_iter().map(|line| format!("host {} disk: {}", survey.here, line.trim_start())).collect()
}

fn lines(survey: &Survey) -> Vec<String> {
    survey
        .hosts
        .iter()
        .filter(|host| host.running.as_ref().is_err_and(|why| why.contains(CLOSED)))
        .flat_map(|host| read_host(survey, host).into_iter().map(|line| format!("host {} disk: {}", host.name, line.trim_start())))
        .collect()
}

fn read_host(survey: &Survey, host: &Host) -> Vec<String> {
    if host.name == survey.here {
        return survey.home.as_deref().map(disk_stores::read).unwrap_or_default();
    }
    let north = north_base::platform::north_binary::north_binary();
    let limit = Duration::from_secs(READS * disk_read::SECONDS.parse::<u64>().unwrap_or_default());
    match north_base::platform::process::run_within(&north.to_string_lossy(), &["runtime", "host-op", "disk", &host.name], limit) {
        Some(output) if output.status.success() => disk_read::over_gib(&String::from_utf8_lossy(&output.stdout)),
        Some(output) => vec![format!("not read: {}", String::from_utf8_lossy(&output.stderr).trim())],
        None => vec!["not read: no answer".to_owned()],
    }
}
