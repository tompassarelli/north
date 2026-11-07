use crate::shell;

fn cpu_property(value: &str) -> bool {
    [
        "CPUQuota=",
        "CPUQuotaPeriodSec=",
        "CPUWeight=",
        "StartupCPUWeight=",
        "AllowedCPUs=",
    ]
    .iter()
    .any(|p| value.starts_with(p))
}

pub(super) fn cpu_file(path: &str) -> bool {
    path.starts_with("/sys/fs/cgroup/")
        && matches!(
            shell::basename(path),
            "cpu.max" | "cpu.weight" | "cpu.weight.nice"
        )
}

pub(super) fn cpu_throttle(words: &[String]) -> bool {
    let args = &words[1..];
    match shell::basename(&words[0]) {
        "systemctl" => {
            args.iter()
                .find(|s| !s.starts_with('-'))
                .is_some_and(|s| s == "set-property")
                && args.iter().any(|s| cpu_property(s))
        }
        "systemd-run" => args
            .iter()
            .take_while(|s| s.as_str() != "--")
            .any(|s| cpu_property(s.trim_start_matches("--property=").trim_start_matches("-p"))),
        "tee" => args.iter().any(|s| cpu_file(s)),
        _ => false,
    }
}

