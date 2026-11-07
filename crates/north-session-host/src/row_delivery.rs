use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use north_base::lifecycle::{Lifecycle, State};

const MARKER: &str = ".handing";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    HandingIn,
    Delivered,
    Interrupted,
    Failed,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::HandingIn, Self::Delivered, Self::Interrupted, Self::Failed];

    fn name(self) -> &'static str {
        match self {
            Self::HandingIn => "handing-in",
            Self::Delivered => "delivered",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

/// One message row on its way into one session's host, kept as `SESSION/ROW`.
pub struct RowDelivery;

impl Lifecycle for RowDelivery {
    const NAME: &'static str = "row-delivery";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        matches!(
            (from, to),
            (None | Some(Phase::Failed | Phase::Interrupted), Phase::HandingIn)
                | (None | Some(Phase::HandingIn | Phase::Interrupted), Phase::Delivered)
                | (Some(Phase::HandingIn), Phase::Failed)
                | (None, Phase::Interrupted)
        )
    }

    fn dwell(state: Phase) -> Option<Duration> {
        match state {
            Phase::HandingIn => Some(Duration::from_secs(10 * 60)),
            Phase::Interrupted => Some(Duration::from_secs(15 * 60)),
            Phase::Failed => Some(Duration::from_secs(60 * 60)),
            Phase::Delivered => None,
        }
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::HandingIn).then_some(Phase::Interrupted)
    }
}

pub fn instance(session: &str, id: &str) -> String {
    format!("{session}/{id}")
}

/// The phase and cause with which each row a session's old journal directory names is adopted: the directory held its entry file and its `.handing` marker, and `took` says whether the session's stream or unaccepted list holds the row.
pub fn adoptions<E: From<io::Error>>(journal: &Path, took: impl Fn(&str) -> Result<bool, E>) -> Result<Vec<(String, Phase, String)>, E> {
    let names: Vec<String> = fs::read_dir(journal)?.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned())).collect::<io::Result<_>>()?;
    let mut ids: Vec<&str> = names.iter().filter(|name| !name.starts_with('.') || name.ends_with(MARKER)).map(|name| name.strip_suffix(MARKER).unwrap_or(name)).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut adopted = Vec::new();
    for id in ids {
        let (entry, handing) = (names.iter().any(|name| name == id), names.iter().any(|name| name.strip_suffix(MARKER) == Some(id)));
        let held = handing && took(id)?;
        let phase = if (entry && !handing) || (handing && held) { Phase::Delivered } else { Phase::Interrupted };
        adopted.push((id.to_owned(), phase, format!("adopted from the journal directory: entry {entry}, hand-in marker {handing}{}", if held { ", row in the stream" } else { "" })));
    }
    Ok(adopted)
}

pub fn drop_old_journal(journal: &Path) -> io::Result<()> {
    for entry in fs::read_dir(journal)? {
        fs::remove_file(entry?.path())?;
    }
    fs::remove_dir(journal)
}
