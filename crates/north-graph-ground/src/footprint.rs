use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use north_graph::{Graph, GraphError, Term};

use crate::show::About;

const PATIENCE: Duration = Duration::from_secs(5);

/// What the graph holds of an issue that describes its work: its title, its boxes' text and the files
/// its landings touched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footprint {
    pub title: String,
    pub boxes: Vec<String>,
    pub touched: BTreeSet<String>,
}

/// Each named issue's footprint, read from the graph at `db`; an issue the graph does not hold is left out.
pub fn footprints(db: &Path, issues: &[String]) -> Result<BTreeMap<String, Footprint>, GraphError> {
    let graph = Graph::open_read_only(db, PATIENCE)?;
    let mut found = BTreeMap::new();
    for issue in issues {
        let about = About(graph.mentions(&Term::string(issue.as_str()), usize::MAX)?);
        let Some(title) = about.first_text(issue, "title") else { continue };
        let mut footprint = Footprint { title: title.to_owned(), ..Footprint::default() };
        for node in about.texts(issue, "has-box") {
            let tell = About(graph.mentions(&Term::string(node), usize::MAX)?);
            footprint.boxes.extend(tell.first_text(node, "text").map(str::to_owned));
        }
        let commits: BTreeSet<&str> = about.texts(issue, "landed-as").chain(about.subjects("checked-for", issue)).collect();
        for commit in commits {
            let seen = About(graph.mentions(&Term::string(commit), usize::MAX)?);
            footprint.touched.extend(seen.texts(commit, "touched").map(str::to_owned));
        }
        found.insert(issue.clone(), footprint);
    }
    Ok(found)
}
