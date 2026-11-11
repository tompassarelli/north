use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::fleet::model::LaunchRequest;

use super::registry::Fleet;

const DIRECTORY: &str = ".launch-queue";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Urgent,
    #[default]
    Normal,
}

impl Priority {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "urgent" => Some(Self::Urgent),
            "normal" => Some(Self::Normal),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::Normal => "normal",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Urgent => 0,
            Self::Normal => 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedLaunch {
    pub name: String,
    #[serde(default)]
    pub priority: Priority,
    pub created_ms: u64,
    pub request: LaunchRequest,
    #[serde(default)]
    pub words: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default)]
    pub take_over: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    pub refused: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tried_ms: Option<u64>,
}

impl QueuedLaunch {
    pub fn new(request: LaunchRequest, priority: Priority, created_ms: u64, refused: String) -> Self {
        Self { name: request.name.clone(), priority, created_ms, request, words: Vec::new(), local: None, pin: None, tier: None, take_over: false, caller: None, refused, tried_ms: None }
    }

    fn file(&self) -> String {
        file_name(&self.name)
    }
}

fn file_name(name: &str) -> String {
    let stem: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{stem}.json")
}

pub struct Admission<'a> {
    pub due: &'a dyn Fn(&QueuedLaunch) -> bool,
    pub gone: &'a dyn Fn(&QueuedLaunch) -> Option<String>,
    pub start: &'a dyn Fn(&QueuedLaunch) -> Result<(), String>,
    pub budget: Duration,
    pub spent: &'a dyn Fn() -> Duration,
}

pub fn order(queued: &mut [QueuedLaunch]) {
    queued.sort_by(|left, right| left.priority.rank().cmp(&right.priority.rank()).then(left.created_ms.cmp(&right.created_ms)).then(left.name.cmp(&right.name)));
}

impl Fleet {
    fn launch_queue(&self) -> PathBuf {
        self.root.join(DIRECTORY)
    }

    pub fn queue_launch(&self, queued: &QueuedLaunch) -> NorthResult<()> {
        let directory = self.launch_queue();
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(queued.file());
        let staged = path.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&staged, serde_json::to_vec(queued)?)?;
        std::fs::rename(staged, path)?;
        Ok(())
    }

    pub fn queued_launches(&self) -> NorthResult<Vec<QueuedLaunch>> {
        let directory = self.launch_queue();
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut queued: Vec<QueuedLaunch> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .filter_map(|entry| std::fs::read(entry.path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()))
            .collect();
        order(&mut queued);
        Ok(queued)
    }

    pub fn remove_queued(&self, name: &str) -> NorthResult<bool> {
        let path = self.launch_queue().join(file_name(name));
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn mark_queued_tried(&self, name: &str, tried_ms: u64) -> NorthResult<()> {
        let mut queued = self.queued_launches()?.into_iter().find(|queued| queued.name == name);
        let Some(mut queued) = queued.take() else { return Ok(()) };
        queued.tried_ms = Some(tried_ms);
        self.queue_launch(&queued)
    }
}

