use std::fs;
use std::path::Path;

use super::{Shape, USAGE};

pub(super) struct Arguments<'a> {
    pub repo: &'a Path,
    pub message: Option<String>,
    pub several_commits: Option<&'a str>,
    pub keep_lane: bool,
    pub wait: bool,
}

impl Arguments<'_> {
    pub(super) fn shape(&self) -> Shape<'_> {
        match self.several_commits {
            Some(reason) => Shape::SeveralCommits { reason },
            None => Shape::OneCommit { message: self.message.as_deref() },
        }
    }
}

pub(super) fn parse(arguments: &[String]) -> Result<Arguments<'_>, u8> {
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return Err(0);
    }
    let (repo, rest) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (Path::new(dir), rest),
        rest => (Path::new("."), rest),
    };
    let (message, rest) = match rest {
        [flag, file, rest @ ..] if flag == "--message" => (Some(read_message(file)?), rest),
        rest => (None, rest),
    };
    let (several_commits, rest) = match rest {
        [flag, reason, rest @ ..] if flag == "--several-commits" && message.is_none() && !reason.trim().is_empty() => (Some(reason.as_str()), rest),
        rest => (None, rest),
    };
    let (keep_lane, rest) = match rest {
        [flag, rest @ ..] if flag == "--keep-lane" => (true, rest),
        rest => (false, rest),
    };
    let (wait, rest) = match rest {
        [flag, rest @ ..] if flag == "--wait" => (true, rest),
        rest => (false, rest),
    };
    if !rest.is_empty() {
        eprintln!("{USAGE}");
        return Err(64);
    }
    Ok(Arguments { repo, message, several_commits, keep_lane, wait })
}

fn read_message(file: &str) -> Result<String, u8> {
    match fs::read_to_string(file) {
        Ok(text) if !text.trim().is_empty() => Ok(text),
        Ok(_) => {
            eprintln!("safe-push: ✗ --message {file} is empty; NOT queued.");
            Err(1)
        }
        Err(error) => {
            eprintln!("safe-push: ✗ cannot read --message {file}: {error}; NOT queued.");
            Err(1)
        }
    }
}
