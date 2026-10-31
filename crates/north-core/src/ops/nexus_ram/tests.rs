use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Inputs, record};

const AT: f64 = 1_792_000_000.0;

fn inputs(root: &Path, now: f64) -> Inputs {
    Inputs { dir: root.join("rec"), proc_root: root.join("proc"), zram: root.join("mm_stat"), now, journal: None }
}

fn write(path: PathBuf, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fake_process(proc_root: &Path, pid: u32, ppid: u32, comm: &str, pss: u32, argv: &[&str]) {
    let dir = proc_root.join(pid.to_string());
    write(dir.join("stat"), &format!("{pid} ({comm}) S {ppid} 1 1 0\n"));
    write(dir.join("cmdline"), &argv.iter().map(|arg| format!("{arg}\0")).collect::<String>());
    write(dir.join("smaps_rollup"), &format!("Rss: 1 kB\nPss: {pss} kB\n"));
}

fn fake_memory(root: &Path) {
    let proc_root = root.join("proc");
    write(proc_root.join("meminfo"), "MemTotal: 8000000 kB\nMemAvailable: 6144000 kB\nSwapTotal: 12000000 kB\nSwapFree: 11900000 kB\n");
    write(proc_root.join("pressure/memory"), "some avg10=0.00 avg60=1.50 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.25 avg300=0.00 total=1\n");
    write(root.join("mm_stat"), "104857600 52428800 62914560 0 70000000 74 0\n");
}

fn rows(root: &Path) -> Vec<Value> {
    let mut files = fs::read_dir(root.join("rec")).unwrap().flatten().map(|entry| entry.path()).collect::<Vec<_>>();
    files.sort();
    files.iter().flat_map(|file| fs::read_to_string(file).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect::<Vec<_>>()).collect()
}

#[test]
fn record_counts_resident_agent_trees_into_one_utc_day_row() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fake_memory(root);
    let proc_root = root.join("proc");
    fake_process(&proc_root, 100, 1, "claude", 204_800, &["/x/bin/claude", "--resume", "abc"]);
    fake_process(&proc_root, 101, 100, "bash", 10_240, &["bash", "-c", "sleep"]);
    fake_process(&proc_root, 104, 101, "claude", 10_240, &["/x/bin/claude", "--nested"]);
    fake_process(&proc_root, 102, 1, "claude", 99_999, &["/x/bin/claude", "-p", "one-shot"]);
    fake_process(&proc_root, 103, 1, "claude", 99_999, &["ugrep", "-G", "pattern"]);
    fake_process(&proc_root, 200, 1, "bash", 1024, &["bash", "/x/codex"]);
    fake_process(&proc_root, 201, 200, "bwrap", 2048, &["bwrap", "--", "codex"]);
    fake_process(&proc_root, 202, 201, "codex", 307_200, &["/x/bin/codex", "-c", "a=b"]);
    fake_process(&proc_root, 203, 202, "codex-code-mode", 51_200, &["/x/codex-code-mode-host"]);
    fake_process(&proc_root, 300, 1, "codex", 99_999, &["/x/bin/codex", "exec", "task"]);
    fake_process(&proc_root, 400, 1, "claude", 99_999, &["/x/bin/claude", "--cloud", "task"]);
    record(&inputs(root, AT)).unwrap();
    let names = fs::read_dir(root.join("rec")).unwrap().flatten().map(|entry| entry.file_name()).collect::<Vec<_>>();
    assert_eq!(names, ["2026-10-14.jsonl"]);
    let text = fs::read_to_string(root.join("rec/2026-10-14.jsonl")).unwrap();
    let expected = r#"{"ts":"2026-10-14T17:46:40Z","mem_available_mib":6000,"swap_used_mib":97,"zram_used_mib":100,"zram_compressed_mib":50,"psi_some_avg60":1.5,"psi_full_avg60":0.25,"agents":2,"agent_pss_mib":570,"gap_s":null,"steal_pct":null,"cpu_stat":null,"cpu_psi":null,"io_psi":null}"#;
    assert_eq!(text, format!("{expected}\n"));
}

fn psi_lines(some: (&str, u64), full: (&str, u64)) -> String {
    format!(
        "some avg10={} avg60=0.00 avg300=0.00 total={}\nfull avg10={} avg60=0.00 avg300=0.00 total={}\n",
        some.0, some.1, full.0, full.1
    )
}

#[test]
fn second_row_holds_steal_share_pressure_deltas_and_gap_since_the_first() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fake_memory(root);
    let proc_root = root.join("proc");
    write(proc_root.join("stat"), "cpu  100 0 50 800 20 0 0 30 0 0\ncpu0 1 2 3 4 5 6 7 8 0 0\nintr 0\n");
    write(proc_root.join("pressure/cpu"), &psi_lines(("1.00", 1_000_000), ("0.00", 200_000)));
    write(proc_root.join("pressure/io"), &psi_lines(("2.00", 5_000_000), ("1.00", 3_000_000)));
    record(&inputs(root, AT)).unwrap();
    write(proc_root.join("stat"), "cpu  300 0 100 1300 20 0 0 280 0 0\ncpu0 9 9 9 9 9 9 9 9 0 0\n");
    write(proc_root.join("pressure/cpu"), &psi_lines(("34.00", 4_500_000), ("12.50", 1_200_000)));
    write(proc_root.join("pressure/io"), &psi_lines(("5.50", 5_250_000), ("3.25", 3_100_000)));
    record(&inputs(root, AT + 95.0)).unwrap();
    let rows = rows(root);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["gap_s"], Value::Null);
    assert_eq!(rows[0]["steal_pct"], Value::Null);
    assert_eq!(rows[0]["cpu_psi"]["some_delta_us"], Value::Null);
    assert_eq!(rows[1]["ts"], "2026-10-14T17:48:15Z");
    assert_eq!(rows[1]["gap_s"], 95);
    assert_eq!(rows[1]["steal_pct"], 25.0);
    assert_eq!(rows[1]["cpu_stat"], json!({"total_jiffies": 2000, "steal_jiffies": 280}));
    let pressure = |some_avg10: f64, full_avg10: f64, totals: [u64; 2], deltas: [u64; 2]| {
        json!({"some_avg10": some_avg10, "full_avg10": full_avg10, "some_total_us": totals[0], "full_total_us": totals[1],
               "some_delta_us": deltas[0], "full_delta_us": deltas[1]})
    };
    assert_eq!(rows[1]["cpu_psi"], pressure(34.0, 12.5, [4_500_000, 1_200_000], [3_500_000, 1_000_000]));
    assert_eq!(rows[1]["io_psi"], pressure(5.5, 3.25, [5_250_000, 3_100_000], [250_000, 100_000]));
}

#[test]
fn first_row_of_a_utc_day_measures_from_the_last_row_of_the_day_before() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fake_memory(root);
    let proc_root = root.join("proc");
    write(proc_root.join("stat"), "cpu  0 0 0 90 0 0 0 10 0 0\n");
    let midnight = 1_792_022_400.0;
    record(&inputs(root, midnight - 30.0)).unwrap();
    write(proc_root.join("stat"), "cpu  0 0 0 180 0 0 0 20 0 0\n");
    record(&inputs(root, midnight + 200.0)).unwrap();
    let rows = rows(root);
    assert_eq!(rows[1]["ts"], "2026-10-15T00:03:20Z");
    assert_eq!(rows[1]["gap_s"], 230);
    assert_eq!(rows[1]["steal_pct"], 10.0);
}

#[test]
fn a_counter_reset_after_reboot_records_null_deltas() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fake_memory(root);
    let proc_root = root.join("proc");
    write(proc_root.join("stat"), "cpu  500 0 0 500 0 0 0 100 0 0\n");
    write(proc_root.join("pressure/cpu"), &psi_lines(("1.00", 9_000_000), ("0.00", 9_000)));
    record(&inputs(root, AT)).unwrap();
    write(proc_root.join("stat"), "cpu  5 0 0 5 0 0 0 1 0 0\n");
    write(proc_root.join("pressure/cpu"), &psi_lines(("1.00", 10), ("0.00", 1)));
    record(&inputs(root, AT + 60.0)).unwrap();
    let rows = rows(root);
    assert_eq!(rows[1]["steal_pct"], Value::Null);
    assert_eq!(rows[1]["cpu_psi"]["some_delta_us"], Value::Null);
    assert_eq!(rows[1]["gap_s"], 60);
}
