//! The dividends' live inputs: dispatch's launch record, the safe-push log, each host's capacity
//! usage log, the cloud credit and GitHub.
use super::checks::{Cpu, DAY_S, Facts, WINDOW_DAYS};
use super::{Bet, Board};
use crate::fleet::landings;
use crate::foundation::clock::Timestamp;
use crate::ops::dispatch;
use crate::ops::pacing::placement;
use crate::platform::{github, host, process::run_within};
use serde_json::Value;
use std::time::Duration;
use toml_edit::{DocumentMut, Item};

const REPO: &str = "tompassarelli/north";
const RELEASE_REPO: &str = "tompassarelli/smashcraft";
const RELEASE_LABEL: &str = "priority:now";
const USAGE_LOG: &str = ".local/state/agents/machine-capacity-usage.jsonl";
const SSH_WAIT: Duration = Duration::from_secs(60);

pub(super) fn facts() -> Facts {
    let at = placement::now();
    let cfg = placement::config();
    let credit = match &cfg {
        Ok(cfg) => placement::cloud_credit(cfg, at),
        Err(error) => (true, format!("policy unread: {error}")),
    };
    Facts {
        at,
        cloud_launches: dispatch::cloud_launch_times(),
        credit,
        cpu: cfg.and_then(|cfg| cpu(&cfg, at)),
        landings: Ok(landings::pushes(&landings::push_log())
            .into_iter()
            .filter(|push| !push.landed.is_empty())
            .map(|push| (push.at_ms as f64 / 1000.0, push.repository))
            .collect()),
        release_closed: release_closed(),
    }
}

pub(super) fn usage_seconds(log: &str, at: f64) -> f64 {
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| {
            row["at"]
                .as_str()
                .and_then(Timestamp::parse_rfc3339)
                .is_some_and(|stamp| at - stamp.unix_seconds_f64() < WINDOW_DAYS * DAY_S)
        })
        .filter_map(|row| row["cpuSeconds"].as_f64())
        .sum()
}

fn read_log(name: &str, here: &str, ssh: Option<&str>) -> Option<String> {
    if name == here {
        let home = std::env::var("HOME").ok()?;
        return std::fs::read_to_string(format!("{home}/{USAGE_LOG}")).ok();
    }
    let output = run_within(
        "ssh",
        &[
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            ssh?,
            &format!("cat ~/{USAGE_LOG}"),
        ],
        SSH_WAIT,
    )?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn cpu(cfg: &DocumentMut, at: f64) -> Result<Cpu, String> {
    let here = host::name();
    let hosts = cfg
        .get("hosts")
        .and_then(Item::as_table_like)
        .ok_or("no [hosts] in the policy")?;
    let mut cpu = Cpu {
        laptop: 0.0,
        total: 0.0,
        unmeasured: Vec::new(),
    };
    let mut laptop_read = false;
    for (name, table) in hosts.iter() {
        let ssh = table.get("ssh").and_then(Item::as_str);
        let laptop = table
            .get("serves")
            .and_then(Item::as_array)
            .is_some_and(|serves| serves.iter().any(|need| need.as_str() == Some("laptop")));
        match read_log(name, &here, ssh) {
            Some(log) => {
                let seconds = usage_seconds(&log, at);
                cpu.total += seconds;
                if laptop {
                    cpu.laptop += seconds;
                    laptop_read = true;
                }
            }
            None => cpu.unmeasured.push(name.to_owned()),
        }
    }
    if laptop_read {
        Ok(cpu)
    } else {
        Err(format!(
            "the laptop's capacity usage log is unreadable (unmeasured: {})",
            cpu.unmeasured.join(", ")
        ))
    }
}

fn release_closed() -> Result<Vec<f64>, String> {
    let since =
        Timestamp::from_unix_seconds(Timestamp::now().unix_seconds() - 7 * 86_400).iso_date();
    let search = format!("closed:>={since}");
    let issues = github::json(&[
        "issue",
        "list",
        "-R",
        RELEASE_REPO,
        "--state",
        "closed",
        "--label",
        RELEASE_LABEL,
        "--search",
        &search,
        "--limit",
        "1000",
        "--json",
        "closedAt",
    ])?;
    Ok(issues
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            Timestamp::parse_rfc3339(issue["closedAt"].as_str()?).map(|at| at.unix_seconds_f64())
        })
        .collect())
}

pub(super) struct Gh;

impl Board for Gh {
    fn bets(&self) -> Result<Vec<Bet>, String> {
        let issues = github::json(&[
            "issue",
            "list",
            "-R",
            REPO,
            "--label",
            "bet",
            "--state",
            "open",
            "--limit",
            "100",
            "--json",
            "number,title,body,comments",
        ])?;
        Ok(issues
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|issue| {
                let comments = issue["comments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|comment| comment["body"].as_str().map(str::to_owned))
                    .collect();
                Some(Bet {
                    number: issue["number"].as_u64()?,
                    title: issue["title"].as_str()?.to_owned(),
                    body: issue["body"].as_str().unwrap_or_default().to_owned(),
                    comments,
                })
            })
            .collect())
    }

    fn comment(&mut self, number: u64, text: &str) -> Result<(), String> {
        let out = github::run(&[
            "issue",
            "comment",
            &number.to_string(),
            "-R",
            REPO,
            "--body",
            text,
        ])
        .map_err(|error| format!("cannot run gh: {error}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "gh issue comment: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    fn alert(&mut self, text: &str) -> Result<(), String> {
        let north = std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "north".into());
        match run_within(
            &north,
            &["fleet", "send", "proxy", text],
            Duration::from_secs(30),
        ) {
            Some(output) if output.status.success() => Ok(()),
            Some(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
            None => Err("north fleet send timed out".into()),
        }
    }
}
