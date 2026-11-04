use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{SlotPool, WasteView};

const STALE_LEASE_SECS: f64 = 3600.0;

const SCRIPT: &str = r#"echo "cores $(nproc)"
head -1 /proc/stat; sleep 1; head -1 /proc/stat
grep -E '^(MemTotal|MemAvailable):' /proc/meminfo
echo "reserve $(tr -d '\n' < "$HOME/.local/state/game-reserve/state.json" 2>/dev/null)"
echo "seen $(cat "$HOME/.local/state/game-reserve/game-seen" 2>/dev/null)"
for lock in "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}"/*.lock; do
  [ -e "$lock" ] || continue
  if flock -n "$lock" true; then echo "slot $(basename "$lock" .lock) free"; else echo "slot $(basename "$lock" .lock) held"; fi
done
ps -wweo etimes=,args= | grep 'slots busy fo[r]' | awk '{print "offload", $1}'
"#;

pub(super) fn read(destination: Option<&str>) -> Option<String> {
    let mut command = Command::new("timeout");
    match destination {
        Some(destination) => command.args(["20", "ssh", "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", destination, "bash", "-s"]),
        None => command.args(["20", "bash", "-s"]),
    };
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    child.stdin.take()?.write_all(SCRIPT.as_bytes()).ok()?;
    let output = child.wait_with_output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn busy_and_total(line: &str) -> Option<(f64, f64)> {
    let fields: Vec<f64> = line.split_whitespace().skip(1).take(8).filter_map(|field| field.parse().ok()).collect();
    let total: f64 = fields.iter().sum();
    let idle = fields.get(3)? + fields.get(4).unwrap_or(&0.0);
    Some((total - idle, total))
}

fn pool_of(slot: &str) -> &str {
    match slot.rsplit_once('-') {
        Some((pool, index)) if !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()) => pool,
        _ => slot,
    }
}

#[derive(Default)]
struct Reading {
    view: WasteView,
    samples: Vec<(f64, f64)>,
    pools: BTreeMap<String, SlotPool>,
    offloads: Vec<u64>,
}

impl Reading {
    fn line(&mut self, line: &str) {
        let Self { view, samples, pools, offloads } = self;
        let (head, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        let kib = || rest.split_whitespace().next().and_then(|amount| amount.parse::<f64>().ok()).map_or(0.0, |amount| amount / 1024.0);
        match head {
            "cores" => view.cores = rest.parse().unwrap_or(0.0),
            "cpu" => samples.extend(busy_and_total(line)),
            "MemTotal:" => view.memory_total_mib = kib(),
            "MemAvailable:" => view.memory_available_mib = kib(),
            "reserve" => view.game_reserve_on = serde_json::from_str::<Value>(rest).is_ok_and(|state| state["on"] == true),
            "seen" => view.game_seen = rest.parse().ok(),
            "slot" => {
                let (slot, state) = rest.split_once(' ').unwrap_or((rest, ""));
                let pool = pools.entry(pool_of(slot).to_owned()).or_insert_with(|| SlotPool { name: pool_of(slot).to_owned(), held: 0, total: 0 });
                pool.total += 1;
                pool.held += usize::from(state == "held");
            }
            "offload" => offloads.extend(rest.parse::<u64>().ok()),
            _ => {}
        }
    }
}

pub(super) fn parse(facts: &str, status: Option<&Value>) -> WasteView {
    let mut reading = Reading::default();
    for line in facts.lines() {
        reading.line(line);
    }
    let Reading { mut view, samples, pools, mut offloads } = reading;
    if let [(busy_then, total_then), (busy_now, total_now)] = samples[..] {
        let spent = total_now - total_then;
        view.busy_cores = if spent > 0.0 { view.cores * (busy_now - busy_then) / spent } else { 0.0 };
    }
    let held: usize = pools.values().map(|pool| pool.held).sum();
    offloads.sort_unstable();
    offloads.truncate(offloads.len().saturating_sub(held));
    view.offloads_waiting = offloads;
    view.slot_pools = pools.into_values().collect();
    if let Some(status) = status {
        view.committed_cpus = status["committedBatchCpus"].as_f64().unwrap_or(0.0);
        view.cpu_limit = status["aggregateCpuLimit"].as_f64().unwrap_or(0.0);
        view.stale_leases = stale_leases(status);
    }
    view
}

fn stale_leases(status: &Value) -> Vec<String> {
    status["holding"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lease| lease["measured"]["liveCores"].as_f64() == Some(0.0) && lease["heldSeconds"].as_f64().unwrap_or(0.0) > STALE_LEASE_SECS && lease["committedCpus"].as_f64().unwrap_or(0.0) > 0.0)
        .map(|lease| format!("{} ({} CPUs, {} h)", lease["owner"].as_str().unwrap_or("unknown"), lease["committedCpus"], (lease["heldSeconds"].as_f64().unwrap_or(0.0) / 3600.0).floor()))
        .collect()
}
