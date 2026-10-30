use super::admission::{Class, Facts, Profile, Reason, admit, decide, default_mib, memory_floor_mib};
use super::native_hold::LeaseFact;
use super::tests::{facts, running};

const TOTAL_MIB: f64 = 96343.0;
const SESSIONS_MIB: f64 = 30000.0;
const REQUEST_MIB: f64 = 8192.0;
const BUILDS: usize = 30;

struct Build {
    threads: f64,
    started: f64,
    seconds: f64,
}

impl Build {
    fn age(&self, now: f64) -> f64 {
        now - self.started
    }

    fn demand(&self, now: f64) -> f64 {
        self.threads * (self.age(now) / 5.0).min(1.0)
    }

    fn resident_mib(&self, now: f64) -> f64 {
        self.threads * 512.0 * (self.age(now) / 60.0).min(1.0)
    }
}

/// A 24-thread host whose CPU some avg10 follows the threads its builds demand and whose MemAvailable follows what they hold.
struct Host {
    cores: f64,
    now: f64,
    pressure: f64,
    running: Vec<Build>,
    started: usize,
}

impl Host {
    fn step(&mut self) {
        self.now += 1.0;
        let now = self.now;
        self.running.retain(|build| build.age(now) < build.seconds);
        let demand: f64 = self.running.iter().map(|build| build.demand(now)).sum();
        let instant = if demand <= self.cores { 2.0 * demand / self.cores } else { 2.0 + 100.0 * (demand - self.cores) / demand };
        self.pressure += (instant - self.pressure) * (1.0 - (-0.1f64).exp());
    }

    fn available_mib(&self) -> f64 {
        TOTAL_MIB - SESSIONS_MIB - self.running.iter().map(|build| build.resident_mib(self.now)).sum::<f64>()
    }

    fn start(&mut self) {
        let threads = 4.0 + (self.started * 5 % 9) as f64;
        self.running.push(Build { threads, started: self.now, seconds: 180.0 + 10.0 * (self.started % 5) as f64 });
        self.started += 1;
    }

    fn facts(&self, profile: Profile, game_cpus: Option<u32>) -> Facts {
        let leases: Vec<LeaseFact> = self.running.iter().map(|build| LeaseFact { created_at: build.started * 1000.0, memory_mib: REQUEST_MIB, ..running("heavy") }).collect();
        Facts {
            profile,
            cores: 24,
            memory_total_mib: TOTAL_MIB,
            memory_available_mib: self.available_mib(),
            cpu_some_avg10_basis_points: (self.pressure * 100.0) as u32,
            cpu_pressure_measured: true,
            leased_memory_mib: REQUEST_MIB * leases.len() as f64,
            leased_batch_cpus: self.running.iter().map(|build| (build.threads * 1.25).max(6.0)).sum(),
            requested_cpus: 6.0,
            requested_memory_mib: REQUEST_MIB,
            game_reserve_agent_cpus: game_cpus,
            gaming: game_cpus.is_some(),
            now: self.now * 1000.0,
            leases,
            ..facts(Class::Heavy)
        }
    }
}

struct Run {
    peak: f64,
    lowest_available_mib: f64,
    waited_under_low: f64,
    longest_wait_under_low: f64,
    seconds_in_band: f64,
    seconds_loaded: f64,
    reached_low_at: Option<f64>,
}

fn simulate(profile: Profile, game_cpus: Option<u32>, low: f64, high: f64) -> Run {
    let (batch_cores, rail) = match (game_cpus, profile) {
        (Some(agent), _) => (f64::from(agent), 1.0),
        (None, Profile::Attended) => (20.0, 1.5),
        (None, Profile::Unattended) => (24.0, 2.0),
    };
    let mut host = Host { cores: game_cpus.map_or(24.0, f64::from), now: 0.0, pressure: 0.0, running: Vec::new(), started: 0 };
    let mut run = Run { peak: 0.0, lowest_available_mib: f64::MAX, waited_under_low: 0.0, longest_wait_under_low: 0.0, seconds_in_band: 0.0, seconds_loaded: 0.0, reached_low_at: None };
    let mut stretch = 0.0;
    while host.started < BUILDS || !host.running.is_empty() {
        host.step();
        let queued = host.started < BUILDS;
        if queued {
            let facts = host.facts(profile, game_cpus);
            let reason = decide(&facts);
            let settling = host.running.iter().any(|build| build.age(host.now) < 10.0);
            let rail_room = facts.leased_batch_cpus + facts.requested_cpus <= batch_cores * rail;
            let floor = if game_cpus.is_some() { default_mib("game_reserve_headroom_mib") } else { memory_floor_mib(profile, TOTAL_MIB) };
            let memory_room = facts.memory_available_mib - REQUEST_MIB - facts.leases.iter().filter(|lease| host.now - lease.created_at / 1000.0 < 60.0).map(|lease| lease.memory_mib).sum::<f64>();
            if reason == Reason::Run {
                host.start();
                stretch = 0.0;
            } else if host.pressure < low && memory_room >= floor && !settling && rail_room {
                run.waited_under_low += 1.0;
                stretch += 1.0;
                run.longest_wait_under_low = f64::max(run.longest_wait_under_low, stretch);
            }
        }
        if run.reached_low_at.is_none() && host.pressure >= low {
            run.reached_low_at = Some(host.now);
        }
        if queued && run.reached_low_at.is_some() {
            run.seconds_loaded += 1.0;
            run.seconds_in_band += f64::from(u8::from(host.pressure >= low && host.pressure <= high));
        }
        run.peak = run.peak.max(host.pressure);
        run.lowest_available_mib = run.lowest_available_mib.min(host.available_mib());
    }
    run
}

#[test]
fn thirty_queued_builds_on_24_threads_hold_each_mode_under_its_high_mark_and_over_its_memory_floor() {
    let run = simulate(Profile::Unattended, None, 20.0, 40.0);
    eprintln!("unattended: peak {:.1}% in band {}/{} s lowest available {:.0} MiB reached low at {:?}", run.peak, run.seconds_in_band, run.seconds_loaded, run.lowest_available_mib, run.reached_low_at);
    assert!(run.reached_low_at.is_some_and(|at| at < 120.0), "the band is reached within two minutes: {:?}", run.reached_low_at);
    assert!(run.peak <= 40.0, "pressure never passes the high mark: {}", run.peak);
    assert!(run.seconds_in_band / run.seconds_loaded >= 0.7, "pressure holds in the band: {}/{}", run.seconds_in_band, run.seconds_loaded);
    assert!(run.lowest_available_mib >= 8192.0, "MemAvailable stays over the unattended floor: {}", run.lowest_available_mib);
    assert_eq!(run.waited_under_low, 0.0, "no ticket waits under the floor with memory room");
    let attended = simulate(Profile::Attended, None, 10.0, 25.0);
    eprintln!("attended: peak {:.1}% in band {}/{} s lowest available {:.0} MiB", attended.peak, attended.seconds_in_band, attended.seconds_loaded, attended.lowest_available_mib);
    assert!(attended.reached_low_at.is_some(), "the attended band is reached");
    assert!(attended.peak <= 25.0, "{}", attended.peak);
    assert!(attended.lowest_available_mib >= TOTAL_MIB / 5.0, "{}", attended.lowest_available_mib);
    assert_eq!(attended.waited_under_low, 0.0);
    let beside = simulate(Profile::Attended, Some(16), 5.0, 20.0);
    eprintln!("game: peak {:.1}% in band {}/{} s lowest available {:.0} MiB", beside.peak, beside.seconds_in_band, beside.seconds_loaded, beside.lowest_available_mib);
    assert!(beside.reached_low_at.is_some(), "the game band is reached");
    assert!(beside.peak <= 20.0, "{}", beside.peak);
    assert!(beside.lowest_available_mib >= 24576.0, "{}", beside.lowest_available_mib);
    assert_eq!(beside.waited_under_low, 0.0);
    assert!(beside.peak < run.peak);
}

#[test]
fn heavy_admission_reads_pressure_the_settle_window_the_overcommit_rail_and_the_band() {
    let heavy = |game_cpus| Facts { leased_batch_cpus: 20.0, requested_cpus: 6.0, game_reserve_agent_cpus: game_cpus, memory_available_mib: 80000.0, ..facts(Class::Heavy) };
    assert_eq!(decide(&heavy(None)), Reason::Run);
    assert_eq!(decide(&heavy(Some(16))), Reason::DeferCpuCapacity);
    let busy = Facts { cpu_some_avg10_basis_points: 6570, cpu_pressure_measured: true, memory_available_mib: 48000.0, need: Some("native:smashcraft".into()), ..facts(Class::Heavy) };
    let advice = admit(&busy, |needs| (needs == "native:smashcraft").then(|| "gex45".to_owned()));
    assert_eq!((advice.reason, advice.offload.as_deref()), (Reason::DeferRunQueue, Some("gex45")));
    let next = |age_seconds: f64| Facts { now: 100_000.0, leases: vec![LeaseFact { created_at: 100_000.0 - age_seconds * 1000.0, ..running("heavy") }], ..facts(Class::Heavy) };
    assert_eq!(decide(&next(9.0)), Reason::DeferCpuPressure);
    assert_eq!(decide(&next(11.0)), Reason::Run);
    let at = |basis_points, leased| Facts { cpu_some_avg10_basis_points: basis_points, leased_batch_cpus: leased, requested_cpus: 6.0, ..facts(Class::Heavy) };
    assert_eq!(decide(&at(500, 20.0)), Reason::Run);
    assert_eq!(decide(&at(1500, 10.0)), Reason::Run);
    assert_eq!(decide(&at(1500, 20.0)), Reason::DeferCpuPressure);
    assert_eq!(decide(&at(500, 30.0)), Reason::DeferCpuCapacity);
    assert_eq!(decide(&at(2500, 0.0)), Reason::DeferCpuPressure);
}
