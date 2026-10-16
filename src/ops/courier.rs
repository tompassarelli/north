use std::path::Path;
use std::time::Duration;

use super::check::Courier;
use crate::platform::process;

const LIMIT: Duration = Duration::from_secs(45);
const TOPIC: &str = "normal";
const NEXUS_HEARTBEAT: &str = "~/.local/state/north-ops/heartbeat";

pub(crate) struct NexusPhone<'a> {
    pub proxy: &'a dyn Fn(&str) -> Result<(), String>,
}

impl Courier for NexusPhone<'_> {
    fn phone(&self, title: &str, text: &str) -> Result<String, String> {
        phone(TOPIC, title, text)
    }

    fn proxy(&self, text: &str) -> Result<(), String> {
        (self.proxy)(text)
    }
}

pub(crate) fn phone(topic: &str, title: &str, text: &str) -> Result<String, String> {
    let local = ["/run/current-system/sw/bin/nexus-alert"].into_iter().find(|path| Path::new(path).exists());
    let output = match local {
        Some(program) => process::run_within(program, &[topic, text, title], LIMIT),
        None => {
            let remote = ["nexus-alert", topic, text, title].map(quote).join(" ");
            process::run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", &remote], LIMIT)
        }
    };
    let output = output.ok_or("nexus-alert timed out or could not start")?;
    let delivery = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    output.status.success().then_some(delivery).ok_or_else(|| format!("nexus-alert exited {}", output.status))
}

pub(crate) fn beat_on_nexus(at: i64) -> Result<(), String> {
    let remote = format!("mkdir -p ~/.local/state/north-ops && printf '%s\\n' {at} > {NEXUS_HEARTBEAT}");
    let output = process::run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", &remote], LIMIT).ok_or("ssh nexus timed out")?;
    output.status.success().then_some(()).ok_or_else(|| format!("ssh nexus exited {}", output.status))
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}
