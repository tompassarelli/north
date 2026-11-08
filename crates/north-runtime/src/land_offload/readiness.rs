use std::process::Command;
use std::time::Duration;

use super::ship::{beside::named_checkouts, quote};
use crate::platform::process::run_command_within;

const CONFIG: &str = ".safe-push";
const REMOTE_HOSTS: &str = "remote-hosts";
const LIMIT: Duration = Duration::from_secs(15);
const MISSING: &str = "missing: ";
const PROBE: &str = r#"for p in @INPUTS@; do case $p in "~/"*) f="$HOME/${p#"~/"}" ;; *) f=$p ;; esac; [ -e "$f" ] || echo "missing: $p"; done
for c in @CHECKOUTS@; do [ -e "$HOME/$c/.git" ] || echo "missing: ~/$c"; done
for a in @ALIASES@; do
  [ "$a" = @HOST@ ] && continue
  h=$(ssh -G "$a" 2>/dev/null | sed -n 's/^hostname //p')
  { [ -n "$h" ] && [ "$h" != "$a" ]; } || getent hosts "$a" >/dev/null 2>&1 || echo "missing: the ssh alias $a"
done
exit 0"#;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Needs {
    checkouts: Vec<String>,
    aliases: Vec<String>,
    inputs: Vec<String>,
}

impl Needs {
    pub fn of(config: &str) -> Self {
        let lines: Vec<&str> = config.lines().filter(|line| !line.trim_start().starts_with('#')).collect();
        let mut aliases: Vec<String> = Vec::new();
        for alias in lines.iter().filter_map(|line| line.trim().strip_prefix(REMOTE_HOSTS)?.trim_start().strip_prefix('=')).flat_map(|value| value.split_whitespace()) {
            if !aliases.iter().any(|seen| seen == alias) {
                aliases.push(alias.to_owned());
            }
        }
        Self { checkouts: named_checkouts(&lines.join("\n")), aliases, inputs: Vec::new() }
    }

    pub fn here() -> Self {
        std::fs::read_to_string(CONFIG).map(|text| Self::of(&text)).unwrap_or_default()
    }

    pub fn with_inputs(self, inputs: &[String]) -> Self {
        Self { inputs: inputs.to_vec(), ..self }
    }

    pub fn lacks_here(&self) -> Vec<String> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        self.inputs.iter().filter(|input| !input.strip_prefix("~/").map_or_else(|| std::path::PathBuf::from(input), |relative| home.join(relative)).exists()).cloned().collect()
    }

    pub fn dialed_from(&self, host: &str, reaches: impl Fn(&str) -> bool) -> Self {
        Self { checkouts: self.checkouts.clone(), aliases: self.aliases.iter().filter(|alias| *alias != host && reaches(alias)).cloned().collect(), inputs: self.inputs.clone() }
    }

    pub fn script(&self, host: &str) -> String {
        let words = |items: &[String]| items.iter().map(|item| quote(item)).collect::<Vec<_>>().join(" ");
        PROBE.replace("@INPUTS@", &words(&self.inputs)).replace("@CHECKOUTS@", &words(&self.checkouts)).replace("@ALIASES@", &words(&self.aliases)).replace("@HOST@", &quote(host))
    }
}

pub fn missing(output: &str) -> Vec<String> {
    output.lines().filter_map(|line| line.strip_prefix(MISSING)).map(str::to_owned).collect()
}

fn probe(needs: &Needs, host: &str, ssh: &str) -> Result<Vec<String>, String> {
    if needs.checkouts.is_empty() && needs.aliases.is_empty() && needs.inputs.is_empty() {
        return Ok(Vec::new());
    }
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &format!("sh -c {}", quote(&needs.script(host)))]);
    let output = run_command_within(command, None, LIMIT).ok_or_else(|| format!("its readiness probe ran past {} s", LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("its readiness probe over ssh exited {}", output.status));
    }
    Ok(missing(&String::from_utf8_lossy(&output.stdout)))
}

pub fn admits(needs: &Needs, host: &str, ssh: &str) -> bool {
    let cfg = crate::platform::orchestration::read().ok();
    let needs = needs.dialed_from(host, |alias| cfg.as_ref().is_some_and(|cfg| crate::platform::orchestration::reaches(cfg, host, alias)));
    match probe(&needs, host, ssh) {
        Ok(lacks) if lacks.is_empty() => true,
        Ok(lacks) => {
            eprintln!("north capacity offload: {host} takes no land check: it lacks {}, which this repository's land checks read", lacks.join(" and "));
            false
        }
        Err(why) => {
            eprintln!("north capacity offload: {host} takes no land check: {why}, so its checkouts and ssh aliases are unknown");
            false
        }
    }
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
