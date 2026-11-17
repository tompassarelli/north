use std::fs;
use std::path::Path;

use super::{Shape, USAGE};

pub(super) struct Arguments<'a> {
    pub repo: &'a Path,
    pub message: Option<String>,
    pub several_commits: Option<&'a str>,
    pub keep_lane: bool,
    pub wait: bool,
    pub check_only: bool,
    pub require_check_only: bool,
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
    let (several_commits, mut rest) = match rest {
        [flag, reason, rest @ ..] if flag == "--several-commits" && message.is_none() && !reason.trim().is_empty() => (Some(reason.as_str()), rest),
        rest => (None, rest),
    };
    let (mut keep_lane, mut wait, mut check_only, mut require_check_only) = (false, false, false, false);
    while let [flag, tail @ ..] = rest {
        let slot = match flag.as_str() {
            "--keep-lane" => &mut keep_lane,
            "--wait" => &mut wait,
            "--check-only" => &mut check_only,
            "--require-check-only" => &mut require_check_only,
            _ => break,
        };
        if *slot {
            eprintln!("{USAGE}");
            return Err(64);
        }
        *slot = true;
        rest = tail;
    }
    if !rest.is_empty() || (check_only && (keep_lane || wait || require_check_only)) {
        eprintln!("{USAGE}");
        return Err(64);
    }
    Ok(Arguments { repo, message, several_commits, keep_lane, wait, check_only, require_check_only })
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
