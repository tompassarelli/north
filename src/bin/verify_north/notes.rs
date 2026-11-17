use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};

use super::processes::Process;

pub enum Note {
    North(PathBuf),
    Directory(PathBuf),
    Scratch(PathBuf),
    State(PathBuf),
    Queue(PathBuf),
    Child(Process),
    Reaped(u32),
}

impl Note {
    fn encode(&self) -> String {
        match self {
            Note::North(path) => format!("north {}", path.display()),
            Note::Directory(path) => format!("directory {}", path.display()),
            Note::Scratch(path) => format!("scratch {}", path.display()),
            Note::State(path) => format!("state {}", path.display()),
            Note::Queue(path) => format!("queue {}", path.display()),
            Note::Child(process) => format!("child {}", process.encode()),
            Note::Reaped(pid) => format!("reaped {pid}"),
        }
    }
}

#[derive(Clone)]
pub struct Notes(Arc<Mutex<Option<ChildStdin>>>);

impl Notes {
    pub fn new(pipe: Option<ChildStdin>) -> Self {
        Self(Arc::new(Mutex::new(pipe)))
    }

    pub fn note(&self, note: &Note) {
        let Ok(mut pipe) = self.0.lock() else { return };
        if let Some(stdin) = pipe.as_mut() {
            let _ = writeln!(stdin, "{}", note.encode()).and_then(|()| stdin.flush());
        }
    }

    pub fn close(&self) {
        if let Ok(mut pipe) = self.0.lock() {
            pipe.take();
        }
    }
}

#[derive(Default)]
pub struct Manifest {
    pub north: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    pub scratch: Option<PathBuf>,
    pub state: Option<PathBuf>,
    pub queue: Option<PathBuf>,
    pub children: BTreeSet<Process>,
}

impl Manifest {
    pub fn read(input: impl BufRead) -> Self {
        let mut manifest = Self::default();
        for line in input.lines().map_while(Result::ok) {
            manifest.apply(&line);
        }
        manifest
    }

    fn apply(&mut self, line: &str) {
        let Some((kind, value)) = line.split_once(' ') else { return };
        match kind {
            "north" => self.north = Some(PathBuf::from(value)),
            "directory" => self.directory = Some(PathBuf::from(value)),
            "scratch" => self.scratch = Some(PathBuf::from(value)),
            "state" => self.state = Some(PathBuf::from(value)),
            "queue" => self.queue = Some(PathBuf::from(value)),
            "child" => self.children.extend(Process::decode(value)),
            "reaped" => self.children.retain(|child| Some(child.pid) != value.parse().ok()),
            _ => {}
        }
    }
}
