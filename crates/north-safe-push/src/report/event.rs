use crate::shell::Shell;

pub struct Event<'a> {
    pub micros: u64,
    pub origin_url: &'a str,
    pub lane: &'a str,
    pub sha: &'a str,
    pub stage: &'a str,
    pub status: &'a str,
    pub detail: &'a str,
    pub tenths: Option<u64>,
    pub pid: u32,
    pub session: &'a str,
    pub host: &'a str,
}

pub fn repository(origin_url: &str) -> &str {
    let name = origin_url.rsplit('/').next().unwrap_or(origin_url);
    name.strip_suffix(".git").unwrap_or(name)
}

pub fn event_line(event: &Event) -> String {
    let seconds = event.tenths.map_or_else(
        || "0".to_owned(),
        |tenths| format!("{}.{}", tenths / 10, tenths % 10),
    );
    format!(
        "{{\"time\":{}.{:06},\"repo\":\"{}\",\"lane\":\"{}\",\"sha\":\"{}\",\"stage\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"seconds\":{},\"pid\":{},\"session\":\"{}\",\"host\":\"{}\"}}\n",
        event.micros / 1_000_000,
        event.micros % 1_000_000,
        json_text(repository(event.origin_url)),
        json_text(event.lane),
        event.sha,
        json_text(event.stage),
        event.status,
        json_text(event.detail),
        seconds,
        event.pid,
        json_text(event.session),
        json_text(event.host),
    )
}

pub fn emit(shell: &Shell, event: &Event) {
    let _ = shell.files.append_event(&event_line(event));
    if matches!(event.status, "refused" | "failed")
        && let Some(watcher) = &shell.settings.watcher
    {
        shell
            .processes
            .spawn_detached(watcher, &["watch", "--once"]);
    }
}

fn json_text(text: &str) -> String {
    text.chars()
        .flat_map(|character| match character {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            '\u{1}'..='\u{1f}' => vec![' '],
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
