use std::fs;
use std::path::{Path, PathBuf};

use super::{features_index, rationale_index};

const USAGE: &str = "usage: north docs features-index|rationale-index [--check] [--root DIR]";

pub struct Index {
    pub command: &'static str,
    pub source_dir: &'static str,
    pub file: &'static str,
    pub render: fn(&Path) -> Result<String, String>,
}

const INDEXES: [&Index; 2] = [&features_index::INDEX, &rationale_index::INDEX];

impl Index {
    pub fn generate(&self, root: &Path) -> Result<(), String> {
        let text = (self.render)(&root.join(self.source_dir))?;
        let index = root.join(self.file);
        if fs::read_to_string(&index).is_ok_and(|current| current == text) {
            return Ok(());
        }
        fs::write(&index, text).map_err(|error| format!("{}: {error}", index.display()))
    }

    pub fn check(&self, root: &Path) -> Result<(), String> {
        let text = (self.render)(&root.join(self.source_dir))?;
        match fs::read_to_string(root.join(self.file)) {
            Ok(current) if current == text => Ok(()),
            _ => Err(format!("{} is stale: run `north docs {}` and commit the result", self.file, self.command)),
        }
    }

    fn find_root(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()?.ancestors().find(|dir| dir.join(self.source_dir).is_dir()).map(Path::to_path_buf)
    }

    fn run(&self, arguments: &[String]) -> Result<(), String> {
        let mut checking = false;
        let mut root = None;
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--check" => checking = true,
                "--root" => root = Some(PathBuf::from(rest.next().ok_or(USAGE)?)),
                _ => return Err(USAGE.to_owned()),
            }
        }
        let root = root.or_else(|| self.find_root()).ok_or_else(|| format!("no {} directory found from the current directory", self.source_dir))?;
        if checking { self.check(&root) } else { self.generate(&root) }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let result = match arguments.split_first() {
        Some((command, rest)) => INDEXES.iter().find(|index| index.command == command).map_or_else(|| Err(USAGE.to_owned()), |index| index.run(rest)),
        None => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            if message == USAGE { 2 } else { 1 }
        }
    }
}
