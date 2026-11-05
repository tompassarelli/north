use crate::capacity_host;

use capacity_host::{Exclusive, Host};
use serde_json::{Value, json};

fn admission(host: &Host, profile: &str, class: &str, cores: u32, total_mib: u32, available_mib: u32) -> Value {
    let (cores, total, available) = (cores.to_string(), total_mib.to_string(), available_mib.to_string());
    let output = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "host", "fixture", "--profile", profile, "--class", class, "--cores", &cores, "--memory-total-mib", &total, "--memory-available-mib", &available])
        .args(["--protected-cpu-some-avg10-basis-points", "0", "--memory-full-avg10-basis-points", "0", "--leased-cpus", "0", "--leased-memory-mib", "0"])
        .output_exclusive()
        .unwrap();
    assert!(matches!(output.status.code(), Some(0 | 75)), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn grant(admission: &Value) -> (&Value, &Value, &Value, Option<&Value>) {
    (&admission["decision"], &admission["cpus"], &admission["memoryMiB"], admission.get("clamped"))
}

#[test]
fn a_request_past_a_small_hosts_grantable_maximum_is_clamped_to_it_and_admitted_when_half_its_memory_is_free() {
    let host = Host::new("capacity-clamp-", &[]);
    let run = json!("RUN");
    assert_eq!(grant(&admission(&host, "attended", "exclusive", 4, 65536, 64000)), (&run, &json!(3), &json!(16384), Some(&json!({ "cpus": [4, 3] }))));
    assert_eq!(grant(&admission(&host, "unattended", "heavy", 4, 8192, 7000)), (&run, &json!(4), &json!(2048), Some(&json!({ "memoryMiB": [8192, 2048] }))));
    for class in ["heavy", "exclusive", "critical"] {
        let small = admission(&host, "unattended", class, 4, 7943, 6038);
        assert_eq!((&small["decision"], &small["cpus"], &small["memoryMiB"]), (&run, &json!(4), &json!(1985)), "a 7943 MiB host idle at 6038 MiB available admits a declared {class} cut to a quarter of RAM");
        assert_eq!(admission(&host, "unattended", class, 4, 7943, 2500)["decision"], json!("DEFER_MEMORY_HEADROOM"), "{class} still waits when the host is short");
    }
    assert_eq!(admission(&host, "unattended", "moderate", 4, 8192, 7000)["decision"], run, "the memory floor never exceeds a quarter of RAM");
    assert_eq!(grant(&admission(&host, "attended", "heavy", 8, 32768, 30000)), (&run, &json!(3), &json!(8192), None), "a laptop-sized host has nothing to clamp");
}
