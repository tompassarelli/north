use std::fmt::Write as _;

use crate::manifest::Task;
use crate::rng::{derive, shuffled};

pub const NOISE_TASKS: usize = 15;
pub const NOISE_RUNS: u32 = 3;
pub const MODEL: &str = "claude-opus-5-5";
pub const EFFORT: &str = "medium";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    T3,
    North,
}

impl Harness {
    pub fn arm_name(self) -> &'static str {
        match self {
            Self::T3 => "t3",
            Self::North => "north",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::T3 => Self::North,
            Self::North => Self::T3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Noise,
    Full,
    Smoke,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Noise => "noise",
            Self::Full => "full",
            Self::Smoke => "smoke",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Trial {
    pub id: String,
    pub phase: Phase,
    pub task: Task,
    pub harness: Harness,
    pub run: u32,
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub index: usize,
    pub trials: Vec<Trial>,
}

impl Unit {
    pub fn is_pair(&self) -> bool {
        self.trials.len() == 2
    }

    pub fn is_large(&self) -> bool {
        self.trials.iter().any(|trial| trial.task.is_large())
    }

    pub fn task(&self) -> &Task {
        &self.trials[0].task
    }
}

#[derive(Debug)]
pub struct Plan {
    pub units: Vec<Unit>,
}

impl Plan {
    pub fn full(master: u64, tasks: &[Task]) -> Self {
        let order = shuffled(tasks, master, &["full-order"]);
        let units = order
            .into_iter()
            .map(|task| {
                let first = if derive(master, &["arm-first", &task.name]) & 1 == 0 {
                    Harness::T3
                } else {
                    Harness::North
                };
                pair(master, Phase::Full, &task, first)
            })
            .collect();
        Self::from_units(units)
    }

    pub fn smoke(master: u64, task: &Task) -> Self {
        let first = if derive(master, &["arm-first", &task.name]) & 1 == 0 {
            Harness::T3
        } else {
            Harness::North
        };
        Self::from_units(vec![pair(master, Phase::Smoke, task, first)])
    }

    pub fn noise(master: u64, tasks: &[Task]) -> Self {
        let subset: Vec<Task> = shuffled(tasks, master, &["noise-subset"])
            .into_iter()
            .take(NOISE_TASKS)
            .collect();
        let mut units = Vec::new();
        for run in 1..=NOISE_RUNS {
            for task in &subset {
                units.push(vec![trial(master, Phase::Noise, task, Harness::T3, run)]);
            }
        }
        Self::from_units(units)
    }

    pub fn campaign(master: u64, tasks: &[Task]) -> Self {
        let mut units = Self::noise(master, tasks).units;
        let full = Self::full(master, tasks).units;
        units.extend(full);
        Self::from_units(units.into_iter().map(|unit| unit.trials).collect())
    }

    fn from_units(units: Vec<Vec<Trial>>) -> Self {
        let units = units
            .into_iter()
            .enumerate()
            .map(|(index, trials)| Unit { index, trials })
            .collect();
        Self { units }
    }

    pub fn trials_of(&self, phase: Phase) -> usize {
        self.units
            .iter()
            .flat_map(|unit| unit.trials.iter())
            .filter(|trial| trial.phase == phase)
            .count()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for unit in &self.units {
            for (position, trial) in unit.trials.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "{:>3} {}.{} seed={:016x} task={} arm={} run={} memory={}MB{}{}",
                    unit.index + 1,
                    position + 1,
                    trial.phase.label(),
                    trial.seed,
                    trial.task.name,
                    trial.harness.arm_name(),
                    trial.run,
                    trial.task.memory_mb,
                    if trial.task.is_large() { " LARGE" } else { "" },
                    if trial.task.is_virtual_machine() {
                        " VM"
                    } else {
                        ""
                    },
                );
            }
        }
        let _ = writeln!(
            out,
            "noise-floor trials: {} ({} seeded tasks x {} plain runs, before the full run)",
            self.trials_of(Phase::Noise),
            NOISE_TASKS,
            NOISE_RUNS
        );
        let _ = writeln!(
            out,
            "planned trials: {} ({} tasks x 2 arms, both arms of a task back to back)",
            self.trials_of(Phase::Full),
            self.units
                .iter()
                .filter(|unit| unit.is_pair() && unit.trials[0].phase == Phase::Full)
                .count()
        );
        out
    }
}

fn pair(master: u64, phase: Phase, task: &Task, first: Harness) -> Vec<Trial> {
    vec![
        trial(master, phase, task, first, 1),
        trial(master, phase, task, first.other(), 1),
    ]
}

fn trial(master: u64, phase: Phase, task: &Task, harness: Harness, run: u32) -> Trial {
    let run_text = run.to_string();
    let seed = derive(
        master,
        &[phase.label(), &task.name, harness.arm_name(), &run_text],
    );
    Trial {
        id: format!(
            "{}-{}-{}-r{}",
            phase.label(),
            task.name,
            harness.arm_name(),
            run
        ),
        phase,
        task: task.clone(),
        harness,
        run,
        seed,
    }
}
