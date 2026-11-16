//! Stand-in staffing (north#1632): a ready box of a repository the orchestration file's `[standin] repos` names goes to
//! a model-free stand-in worker on the first `[standin] hosts` host with a free slot, so a proof run drives dispatch,
//! the hand-in and the lander at a set rate with no tokens spent.
use toml_edit::{DocumentMut, Item};

use super::issues::Issue;
use super::plan::{Candidate, Launch, Lineage, Ready};
use super::slots::Pools;

pub const ACCOUNT: &str = "standin:local";
const BOX: &str = "standin ";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standin {
    repos: Vec<String>,
    hosts: Vec<String>,
}

impl Standin {
    pub fn parse(cfg: &DocumentMut) -> Self {
        let strings = |key: &str| -> Vec<String> { cfg.get("standin").and_then(|table| table.get(key)).and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect() };
        Self { repos: strings("repos"), hosts: strings("hosts") }
    }

    pub fn takes(&self, issue: &Issue) -> bool {
        self.repos.iter().any(|repo| repo.eq_ignore_ascii_case(&issue.reference().repo))
    }

    /// Places `ready` on the first stand-in host that serves the box and has a free slot, taking that slot.
    pub fn place(&self, ready: &Ready, box_text: Option<&str>, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
        if !box_text.is_some_and(|text| text.trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").trim().starts_with(BOX)) {
            return Err(format!("a stand-in repository staffs only boxes starting `{BOX}`, which north proof-run print writes"));
        }
        let serves = |host: &str| candidates.iter().any(|candidate| candidate.host == host && candidate.refusal.is_none());
        let host = self.hosts.iter().find(|host| serves(host) && pools.host(host) > 0).ok_or_else(|| format!("no stand-in host ({}) serves this box with a free slot", self.hosts.join(", ")))?;
        pools.take(host);
        let local = Some(format!("north fleet dispatch: a stand-in takes this box on {host} (north#1632)"));
        Ok(Launch { item: ready.item.clone(), host: host.clone(), account: ACCOUNT.into(), brief: ready.brief.clone(), route: ready.route.clone(), local, lineage: Lineage::default() })
    }
}

pub fn is(launch: &Launch) -> bool {
    launch.account == ACCOUNT
}
