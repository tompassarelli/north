use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};

use super::processes::Process;

pub enum Note {
    North(PathBuf),
    Directory(PathBuf),
    Session(String),
    Scratch(PathBuf),
    Queue(PathBuf),
    Child(Process),
    Reaped(u32),
    Watch(Process),
}

impl Note {
    fn encode(&self) -> String {
        match self {
            Note::North(path) => format!("north {}", path.display()),
            Note::Directory(path) => format!("directory {}", path.display()),
            Note::Session(name) => format!("session {name}"),
            Note::Scratch(path) => format!("scratch {}", path.display()),
            Note::Queue(path) => format!("queue {}", path.display()),
            Note::Child(process) => format!("child {}", process.encode()),
            Note::Reaped(pid) => format!("reaped {pid}"),
            Note::Watch(process) => format!("watch {}", process.encode()),
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

    pub fn watch_all(&self, processes: impl IntoIterator<Item = Process>) {
        for process in processes {
            self.note(&Note::Watch(process));
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
    pub sessions: Vec<String>,
    pub scratch: Option<PathBuf>,
    pub queue: Option<PathBuf>,
    pub children: BTreeSet<Process>,
    pub watched: BTreeSet<Process>,
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
            "session" => self.sessions.push(value.to_owned()),
            "scratch" => self.scratch = Some(PathBuf::from(value)),
            "queue" => self.queue = Some(PathBuf::from(value)),
            "child" => self.children.extend(Process::decode(value)),
            "reaped" => self.children.retain(|child| Some(child.pid) != value.parse().ok()),
            "watch" => self.watched.extend(Process::decode(value)),
            _ => {}
        }
    }
}
