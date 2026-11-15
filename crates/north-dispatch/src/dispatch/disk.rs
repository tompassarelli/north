use super::record;
use super::slots::Host;
use super::survey::Survey;
use north_base::platform::disk_read;
use std::time::Duration;

const READS: u64 = 6;
const KEPT: &str = "disk-read.txt";
const FRESH: Duration = Duration::from_secs(3600);

/// Prints the directories of at least 1 GiB under the fixed roots of each host dispatch skipped under its disk bound, largest first, after the run has decided and started its work. A dry run reads them afresh; a real run reuses the lines it kept within the hour, so a host that stays closed does not cost a `du` on every timer run.
pub(super) fn print(survey: &Survey, dry_run: bool) {
    let kept = record::directory().join(KEPT);
    let fresh = !dry_run && std::fs::metadata(&kept).and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < FRESH);
    let lines = match std::fs::read_to_string(&kept).ok().filter(|_| fresh) {
        Some(text) => text.lines().map(str::to_owned).collect(),
        None => {
            let lines = lines(survey);
            if !dry_run && !lines.is_empty() {
                let _ = std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::write(&kept, lines.join("\n")));
            }
            lines
        }
    };
    for line in lines {
        println!("{line}");
    }
}

fn lines(survey: &Survey) -> Vec<String> {
    survey
        .hosts
        .iter()
        .filter(|host| host.running.as_ref().is_err_and(|why| why.contains("disk bound")))
        .flat_map(|host| read(survey, host).into_iter().map(|line| format!("host {} disk: {}", host.name, line.trim_start())))
        .collect()
}

fn read(survey: &Survey, host: &Host) -> Vec<String> {
    if host.name == survey.here {
        return survey.home.as_deref().map(disk_read::local).unwrap_or_default();
    }
    let north = north_base::platform::north_binary::north_binary();
    let limit = Duration::from_secs(READS * disk_read::SECONDS.parse::<u64>().unwrap_or_default());
    match north_base::platform::process::run_within(&north.to_string_lossy(), &["runtime", "host-op", "disk", &host.name], limit) {
        Some(output) if output.status.success() => disk_read::over_gib(&String::from_utf8_lossy(&output.stdout)),
        Some(output) => vec![format!("not read: {}", String::from_utf8_lossy(&output.stderr).trim())],
        None => vec!["not read: no answer".to_owned()],
    }
}
