//! The hosts a box's `Needs:` line names, and whether a target host can reach them: a `[hosts.NAME]`
//! word that is not a local need must be the target itself or in the target's `reaches` list; a host
//! that declares no list reaches any.
use super::gate::{is_local_need, words};
use super::strings;
use toml_edit::{DocumentMut, Item};

pub(super) struct Reach {
    named: Vec<String>,
    here: String,
    here_reaches: Option<Vec<String>>,
}

impl Reach {
    pub(super) fn of(text: &str, cfg: &DocumentMut, here: &str) -> Self {
        let hosts = cfg.get("hosts");
        let named = words(text, "Needs").into_iter().filter(|word| !is_local_need(word) && hosts.is_some_and(|hosts| hosts.get(word).is_some())).collect();
        let here_reaches = hosts.and_then(|hosts| hosts.get(here)).and_then(reaches);
        Self { named, here: here.to_owned(), here_reaches }
    }

    pub(super) fn named(&self) -> &[String] {
        &self.named
    }

    /// The first named host `host` cannot reach, None when it reaches them all.
    pub(super) fn unreached(&self, host: &str, reaches: Option<&[String]>) -> Option<&str> {
        self.named.iter().map(String::as_str).find(|named| *named != host && reaches.is_some_and(|reaches| !reaches.iter().any(|reached| reached == named)))
    }

    pub(super) fn unreached_here(&self) -> Option<&str> {
        self.unreached(&self.here, self.here_reaches.as_deref())
    }
}

/// The hosts a host declares it reaches, None when it declares no list.
pub(super) fn reaches(host: &Item) -> Option<Vec<String>> {
    host.get("reaches").map(|_| strings(host.get("reaches")))
}
