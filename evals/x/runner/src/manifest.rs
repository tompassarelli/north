use crate::error::{Result, RunError};

pub const LARGE_MEMORY_MB: u32 = 8192;
pub const TASKS_COMMIT: &str = "2fd12b88aafdd04a52c298e3940bcb189f9766d6";
pub const HARBOR_COMMIT: &str = "fdba28fc411e47e8765420a1a063b4918fce3def";

const VIRTUAL_MACHINE_TASKS: [&str; 3] =
    ["install-windows-3.11", "qemu-alpine-ssh", "qemu-startup"];
const TABLE: &str = include_str!("../tasks.tsv");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub memory_mb: u32,
    pub image: String,
    pub agent_timeout_s: u32,
}

impl Task {
    pub fn image_ref(&self) -> String {
        format!("docker.io/{}", self.image)
    }

    pub fn is_large(&self) -> bool {
        self.memory_mb >= LARGE_MEMORY_MB
    }

    pub fn is_virtual_machine(&self) -> bool {
        VIRTUAL_MACHINE_TASKS.contains(&self.name.as_str())
    }
}

pub fn all() -> Result<Vec<Task>> {
    let mut tasks = parse(TABLE)?;
    tasks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tasks)
}

pub fn named(name: &str) -> Result<Task> {
    all()?
        .into_iter()
        .find(|task| task.name == name)
        .ok_or_else(|| RunError::Usage(format!("no task named {name} in the pinned table")))
}

fn parse(table: &str) -> Result<Vec<Task>> {
    table
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                [name, memory, image, timeout] => Ok(Task {
                    name: (*name).to_owned(),
                    memory_mb: memory
                        .parse()
                        .map_err(|_| RunError::Failed(format!("bad memory in {line}")))?,
                    image: (*image).to_owned(),
                    agent_timeout_s: timeout
                        .parse()
                        .map_err(|_| RunError::Failed(format!("bad timeout in {line}")))?,
                }),
                _ => Err(RunError::Failed(format!(
                    "bad row in the task table: {line}"
                ))),
            }
        })
        .collect()
}
