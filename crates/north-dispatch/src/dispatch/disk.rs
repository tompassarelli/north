use super::disk_stores;
use super::record;
use super::slots::Host;
use super::survey::Survey;
use north_base::platform::disk_read;
use std::time::Duration;

const READS: u64 = 6;
const KEPT: &str = "disk-read.txt";
const FRESH: Duration = Duration::from_secs(3600);
const CLOSED: &str = "free on the root disk";

/// The disk lines of each host dispatch skipped for its root disk, under the build floor or its disk bound, after the run has decided and started its work: this host's harness stores by size, after the harness's own disk keep has run here, and the directories of at least 1 GiB under the fixed roots of any other. A dry run reads afresh and keeps nothing; a real run reuses the lines it kept within the hour, so a host that stays closed costs one read and one keep an hour.
pub(super) fn read(survey: &Survey, dry_run: bool) -> Vec<String> {
    let kept = record::directory().join(KEPT);
    let fresh = !dry_run && std::fs::metadata(&kept).and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < FRESH);
    if let Some(text) = std::fs::read_to_string(&kept).ok().filter(|_| fresh) {
        return text.lines().map(str::to_owned).collect();
    }
    let lines = lines(survey, dry_run);
    if !dry_run && !lines.is_empty() {
        let _ = std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::write(&kept, lines.join("\n")));
    }
    lines
}

/// This host's disk lines: those `read` kept when this host is closed for its disk, else a fresh read of its harness stores.
pub(super) fn here(survey: &Survey, kept: &[String]) -> Vec<String> {
    let prefix = format!("host {} disk: ", survey.here);
    let lines: Vec<String> = kept.iter().filter(|line| line.starts_with(&prefix)).cloned().collect();
    if !lines.is_empty() {
        return lines;
    }
    survey.home.as_deref().map(disk_stores::read).unwrap_or_default().into_iter().map(|line| format!("{prefix}{}", line.trim_start())).collect()
}

fn lines(survey: &Survey, dry_run: bool) -> Vec<String> {
    survey
        .hosts
        .iter()
        .filter(|host| host.running.as_ref().is_err_and(|why| why.contains(CLOSED)))
        .flat_map(|host| read_host(survey, host, dry_run).into_iter().map(|line| format!("host {} disk: {}", host.name, line.trim_start())))
        .collect()
}

fn read_host(survey: &Survey, host: &Host, dry_run: bool) -> Vec<String> {
    if host.name == survey.here {
        let Some(home) = survey.home.as_deref() else { return Vec::new() };
        let kept = if dry_run { vec!["keep: not run in a dry run".to_owned()] } else { disk_stores::keep(home) };
        return kept.into_iter().chain(disk_stores::read(home)).collect();
    }
    let north = north_base::platform::north_binary::north_binary();
    let limit = Duration::from_secs(READS * disk_read::SECONDS.parse::<u64>().unwrap_or_default());
    match north_base::platform::process::run_within(&north.to_string_lossy(), &["runtime", "host-op", "disk", &host.name], limit) {
        Some(output) if output.status.success() => disk_read::over_gib(&String::from_utf8_lossy(&output.stdout)),
        Some(output) => vec![format!("not read: {}", String::from_utf8_lossy(&output.stderr).trim())],
        None => vec!["not read: no answer".to_owned()],
    }
}
