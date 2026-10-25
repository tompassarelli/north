use std::os::unix::process::CommandExt;
use std::process::Command;

use rustix::process::{CpuSet, sched_getaffinity, sched_setaffinity};

const USAGE: &str = "usage: north capacity pin CPU_LIST -- COMMAND...";

pub(crate) fn run(arguments: &[String]) -> u8 {
    let [list, separator, program, rest @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if separator != "--" {
        eprintln!("{USAGE}");
        return 2;
    }
    let allowed = match sched_getaffinity(None) {
        Ok(allowed) => allowed,
        Err(error) => {
            eprintln!("north capacity pin: {error}");
            return 1;
        }
    };
    let set = match cpu_set(list, |cpu| allowed.is_set(cpu)) {
        Ok(set) => set,
        Err(error) => {
            eprintln!("north capacity pin: {error}");
            return 2;
        }
    };
    if let Err(error) = sched_setaffinity(None, &set) {
        eprintln!("north capacity pin: {error}");
        return 1;
    }
    let error = Command::new(program).args(rest).exec();
    eprintln!("north capacity pin: {program}: {error}");
    127
}

fn cpus(list: &str) -> Result<Vec<usize>, String> {
    let mut cpus = Vec::new();
    for range in list.split(',') {
        let (low, high) = range.split_once('-').unwrap_or((range, range));
        let parse = |text: &str| text.parse::<usize>().ok().filter(|_| text.bytes().all(|byte| byte.is_ascii_digit()));
        let (Some(low), Some(high)) = (parse(low), parse(high)) else {
            return Err(format!("{list} is not a CPU list such as 0-11 or 0,2,4"));
        };
        if low > high {
            return Err(format!("range {range} must run low-high"));
        }
        if high >= CpuSet::MAX_CPU {
            return Err(format!("CPU {high} is outside this process's allowed CPUs"));
        }
        cpus.extend(low..=high);
    }
    Ok(cpus)
}

fn cpu_set(list: &str, allowed: impl Fn(usize) -> bool) -> Result<CpuSet, String> {
    let mut set = CpuSet::new();
    for cpu in cpus(list)? {
        if cpu >= CpuSet::MAX_CPU || !allowed(cpu) {
            return Err(format!("CPU {cpu} is outside this process's allowed CPUs"));
        }
        set.set(cpu);
    }
    Ok(set)
}
