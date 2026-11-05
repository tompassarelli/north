use super::measure;

#[test]
fn a_hosts_measure_is_read_from_nproc_loadavg_and_meminfo() {
    let probe = "20\n41.02 37.72 35.15 31/5160 3351912\nMemTotal:       65621040 kB\nMemFree:        12404044 kB\nMemAvailable:   31894972 kB\n";
    let read = measure::parse(probe).unwrap();
    assert_eq!((read.cores, read.load), (20.0, 41.02));
    assert!((read.memory_total_mib - 64083.04).abs() < 0.01 && (read.memory_available_mib - 31147.43).abs() < 0.01, "{read:?}");
    assert_eq!(measure::parse("20\n"), None);
}
