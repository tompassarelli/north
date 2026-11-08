use super::{OTHER_JOBS, SLOTS};

/// The offload jobs of the lander's 8:33 AM round on 2026-11-08, from its spans: probe, kind (B builds head-north, C one check, H the head-north batch that starts when its probe's build ends), recorded host, arrival in seconds after 01:38:00 UTC, and run seconds once in a slot.
const ROUND: &str = "4e7420 B cpx62 6 2|4e7420 C gex45 8 60|4e7420 C gex45 8 91|4e7420 C cpx62 8 87|4e7420 H cpx62 0 36|f4fe7a B cpx62 7 2|f4fe7a C cpx62 8 167|f4fe7a C gex45 8 43|f4fe7a C gex45 8 82|f4fe7a H cpx62 0 25|49b25e B gex45 20 54|49b25e C gex45 22 23|49b25e H gex45 0 100|7e98d5 B gex45 27 112|7e98d5 C gex45 28 21|7e98d5 C gex45 28 56|7e98d5 C gex45 28 116|7e98d5 C gex45 28 96|7e98d5 H gex45 0 69|a9c998 B gex45 47 22|a9c998 C gex45 48 244|a9c998 C gex45 48 73|a9c998 C gex45 48 72|a9c998 C gex45 48 43|a9c998 H gex45 0 48";

/// The lane checks that held code-north-main slots in that round, from the slot holder lines its probes logged: arrival and run seconds, each on gex45.
const LANES: [(u32, u32); 14] = [(0, 9), (7, 5), (7, 5), (9, 108), (11, 95), (42, 48), (90, 6), (90, 39), (106, 182), (117, 23), (143, 73), (248, 42), (288, 60), (298, 60)];

/// The probes of the round: 8 planned on 8 free slots, of which 5 offloaded and 3 were refused before any check.
const PLANNED: usize = 8;
/// The median offload jobs per probe in the round's probe logs.
const JOBS_PER_PROBE: usize = 7;
const HOSTS: [(&str, u32); 2] = [("gex45", 5), ("cpx62", 4)];

#[derive(Clone, Copy)]
enum Rules {
    Before,
    After,
}

struct Job {
    probe: Option<&'static str>,
    host: usize,
    build: bool,
    batch: bool,
    at: Option<u32>,
    run: u32,
    started: Option<u32>,
    reserved: bool,
}

impl Job {
    fn lander(&self) -> bool {
        self.probe.is_some()
    }

    fn holds(&self, t: u32) -> bool {
        self.started.is_some_and(|started| t < started + self.run)
    }

    fn queued(&self, t: u32) -> bool {
        self.started.is_none() && self.at.is_some_and(|at| at <= t)
    }
}

fn round(probes: &[&str]) -> Vec<Job> {
    let lander = ROUND.split('|').map(|row| row.split(' ').collect::<Vec<_>>()).filter(|words| probes.contains(&words[0])).map(|words| {
        let (kind, at, run) = (words[1], words[3].parse().unwrap(), words[4].parse().unwrap());
        Job { probe: Some(words[0]), host: HOSTS.iter().position(|(name, _)| *name == words[2]).unwrap(), build: kind == "B", batch: kind == "H", at: (kind != "H").then_some(at), run, started: None, reserved: false }
    });
    lander.chain(LANES.iter().map(|&(at, run)| Job { probe: None, host: 0, build: false, batch: false, at: Some(at), run, started: None, reserved: false })).collect()
}

/// Runs the jobs second by second through the slot script's rules on each host: a lander job takes the reserved slot when free, else a slot under the job cap once first in line; `Before` ranks lane checks with the lander's probes, `After` ranks the lander's first and admits lane checks only while they hold fewer than `OTHER_JOBS` of its slots. Returns the lander's job-seconds spent waiting while a lane check held a slot of its host, and its total wait.
fn replay(jobs: &mut [Job], rules: Rules, placed: bool) -> (u32, u32) {
    let (mut behind, mut waited) = (0, 0);
    for t in 0..3000 {
        for index in 0..jobs.len() {
            if jobs[index].batch && jobs[index].at.is_none() {
                let built = jobs.iter().find(|job| job.build && job.probe == jobs[index].probe).and_then(|build| build.started.map(|started| started + build.run + 1));
                jobs[index].at = built.filter(|at| *at <= t);
            }
            if !placed && jobs[index].lander() && jobs[index].queued(t) && jobs[index].started.is_none() && jobs[index].at == Some(t) {
                jobs[index].host = (0..HOSTS.len()).max_by_key(|&host| lander_room(jobs, host, t)).unwrap_or(0);
            }
        }
        for (host, &(_, cap)) in HOSTS.iter().enumerate() {
            let mut line: Vec<usize> = (0..jobs.len()).filter(|&index| jobs[index].host == host && jobs[index].queued(t)).collect();
            line.sort_by_key(|&index| (matches!(rules, Rules::After) && !jobs[index].lander(), jobs[index].at));
            for index in line {
                let held = jobs.iter().filter(|job| job.host == host && job.holds(t)).count() as u32;
                let others = jobs.iter().filter(|job| job.host == host && !job.lander() && job.holds(t)).count() as u32;
                let reserved_free = !jobs.iter().any(|job| job.host == host && job.reserved && job.holds(t));
                let ahead = jobs.iter().enumerate().any(|(other, job)| other != index && job.host == host && job.queued(t) && earlier(job, &jobs[index], rules) && !(matches!(rules, Rules::After) && !job.lander() && others >= OTHER_JOBS));
                let lander = jobs[index].lander();
                if lander && reserved_free {
                    jobs[index].reserved = true;
                } else if ahead || held >= cap.min(SLOTS) || (matches!(rules, Rules::After) && !lander && others >= OTHER_JOBS) {
                    continue;
                }
                jobs[index].started = Some(t);
            }
        }
        for job in jobs.iter().filter(|job| job.lander() && job.queued(t)) {
            waited += 1;
            behind += u32::from(jobs.iter().any(|lane| !lane.lander() && lane.host == job.host && lane.holds(t)));
        }
    }
    assert!(jobs.iter().all(|job| job.started.is_some()), "every job ran within the replay");
    (behind, waited)
}

fn earlier(job: &Job, than: &Job, rules: Rules) -> bool {
    let rank = |job: &Job| u8::from(matches!(rules, Rules::After) && !job.lander());
    (rank(job), job.at) < (rank(than), than.at)
}

fn lander_room(jobs: &[Job], host: usize, t: u32) -> i64 {
    let cap = HOSTS[host].1;
    let lander = jobs.iter().filter(|job| job.host == host && job.lander() && (job.holds(t) || job.queued(t))).count() as i64;
    i64::from(cap - OTHER_JOBS) - lander
}

// KEEP(bug): in the 8:33 AM round of 2026-11-08 the lander planned 8 probes of about 7 offload jobs on 8 free slots, and its jobs waited 28 to 154 s for code-north-main slots, partly behind lane checks ranked level with them
#[test]
fn a_replay_of_the_833_am_round_plans_only_the_probes_whose_jobs_fit_the_landers_slots_and_no_probe_job_waits_behind_a_lane_check() {
    let probes = ["4e7420", "f4fe7a", "49b25e", "7e98d5", "a9c998"];
    let (behind, waited) = replay(&mut round(&probes), Rules::Before, true);
    assert!(PLANNED * JOBS_PER_PROBE > 8 && behind > 0, "the recorded round oversubscribed its slots and its probe jobs waited behind lane checks: {behind} of {waited} job-seconds");
    let lander_slots: usize = HOSTS.iter().map(|&(_, cap)| (cap - OTHER_JOBS) as usize).sum();
    let planned = (lander_slots / JOBS_PER_PROBE).max(1);
    assert!(planned * JOBS_PER_PROBE <= lander_slots, "{planned} probes of {JOBS_PER_PROBE} jobs fit the lander's {lander_slots} slots");
    let longest = probes.into_iter().max_by_key(|probe| ROUND.split('|').filter(|row| row.starts_with(probe)).count()).unwrap();
    let chosen: Vec<&str> = std::iter::once(longest).take(planned).collect();
    let (behind, waited) = replay(&mut round(&chosen), Rules::After, false);
    assert_eq!(behind, 0, "no probe job of the planned round waits while a lane check holds a slot of its host ({waited} job-seconds waited in all)");
}
