use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use north_github_mirror::read::{Seen, recorded_issue_state};
use north_github_mirror::store::Mirror;
use regex::Regex;

use super::{Range, git};

const KINDS: [&str; 9] = ["plan", "plans", "planning", "port", "porting", "migration", "migrations", "order", "orders"];

static OWNERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*(?:[-*]\s*)?(?:\*\*)?owners?(?:\*\*)?:\s*(.*)$").expect("owners pattern"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:\b([a-z][a-z0-9_-]*))?#(\d+)").expect("issue reference pattern"));
static CREATES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*(?:[-*]\s*)?creates:.*$").expect("creates pattern"));
static BACKTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`\s]+)`").expect("backtick pattern"));
static LOCATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?::\d+(?:-\d+)?|::.*)$").expect("location suffix pattern"));

struct Tree {
    files: BTreeSet<String>,
    tops: BTreeSet<String>,
}

impl Tree {
    fn new(listing: &str) -> Self {
        let files: BTreeSet<String> = listing.lines().map(str::to_owned).collect();
        let tops = files.iter().filter_map(|path| path.split('/').next()).map(str::to_owned).collect();
        Self { files, tops }
    }

    fn contains(&self, path: &str) -> bool {
        let directory = format!("{}/", path.trim_end_matches('/'));
        self.files.contains(path) || self.files.range(directory.clone()..).next().is_some_and(|next| next.starts_with(&directory))
    }
}

fn is_plan_document(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("docs/").filter(|rest| !rest.starts_with("references/") && !rest.starts_with("features/")) else {
        return false;
    };
    let Some(stem) = Path::new(rest).file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".md")) else {
        return false;
    };
    stem.split(|character: char| !character.is_ascii_alphanumeric()).any(|word| KINDS.contains(&word.to_ascii_lowercase().as_str()))
}

fn owners(text: &str, own: &str) -> Vec<(String, u64)> {
    OWNERS
        .captures_iter(text)
        .flat_map(|line| {
            REFERENCE
                .captures_iter(&line[1])
                .filter_map(|found| Some((found.get(1).map_or(own, |repository| repository.as_str()).to_ascii_lowercase(), found[2].parse().ok()?)))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn missing_paths(text: &str, tree: &Tree) -> Vec<String> {
    let text = CREATES.replace_all(text, "");
    let mut missing: Vec<String> = BACKTICKED
        .captures_iter(&text)
        .map(|found| LOCATION.replace(found[1].trim_end_matches(['.', ',', ';']), "").into_owned())
        .filter(|token| {
            let trailing = token.ends_with('/');
            let last = token.rsplit('/').find(|part| !part.is_empty()).unwrap_or_default();
            let plain = token.chars().all(|character| character.is_ascii_alphanumeric() || "/._-@".contains(character)) && !token.starts_with(['/', '.']);
            plain && token.contains('/') && (trailing || last.contains('.')) && token.split('/').next().is_some_and(|top| tree.tops.contains(top))
        })
        .filter(|token| !tree.contains(token))
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

fn findings(path: &str, text: &str, tree: &Tree, own: &str, touched: bool, state: &dyn Fn(&str, u64) -> Option<Seen>) -> Vec<String> {
    let mut found = Vec::new();
    let owned = owners(text, own);
    if owned.is_empty() {
        if touched {
            found.push(format!("{path}: a plan, port, migration or order document names no owning issue; add a line `Owners: {own}#N` for the open issue that finishes it"));
        }
    } else if owned.iter().all(|(repository, number)| state(repository, *number) == Some(Seen::Closed)) {
        let names: Vec<String> = owned.iter().map(|(repository, number)| format!("{repository}#{number}")).collect();
        found.push(format!("{path}: every owner ({}) is closed in the GitHub mirror; delete the document, or rewrite it to describe what shipped under a name without plan, port, migration or order", names.join(", ")));
    }
    if touched {
        for missing in missing_paths(text, tree) {
            found.push(format!("{path}: names `{missing}`, which does not exist; delete the line, or mark a file the work will create on a `Creates:` line"));
        }
    }
    found
}

fn origin_name(directory: &Path) -> String {
    let url = git::text(directory, &["remote", "get-url", "origin"]).unwrap_or_default();
    let name = url.trim().rsplit(['/', ':']).next().unwrap_or_default();
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

pub fn scan(directory: &Path, range: &str, mirror: &Mirror, out: &mut impl Write) -> u8 {
    let Some(Range { base, head: revision }) = Range::parse(range) else {
        let _ = writeln!(out, "docs-owners: expected BASE..HEAD, got {range}");
        return 2;
    };
    let fork = git::merge_base(directory, base, revision);
    let changed: BTreeSet<String> = git::text(directory, &["diff", "--name-only", "--diff-filter=d", &fork, revision, "--"]).unwrap_or_default().lines().map(str::to_owned).collect();
    let Some(listing) = git::text(directory, &["ls-tree", "-r", "--name-only", revision]) else {
        let _ = writeln!(out, "docs-owners: cannot list {revision}");
        return 1;
    };
    let tree = Tree::new(&listing);
    let own = origin_name(directory);
    let state = |repository: &str, number: u64| recorded_issue_state(mirror, repository, number);
    let mut refused = false;
    for path in tree.files.iter().filter(|path| is_plan_document(path)) {
        let Some(text) = git::text(directory, &["show", &format!("{revision}:{path}")]) else {
            continue;
        };
        for finding in findings(path, &text, &tree, &own, changed.contains(path.as_str()), &state) {
            refused = true;
            let _ = writeln!(out, "docs-owners: {finding}");
        }
    }
    if refused {
        let _ = writeln!(out, "docs-owners: a document of planned or ported work does not outlive its issues (north#635)");
    }
    u8::from(refused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_documents_name_open_owners_and_existing_paths() {
        let tree = Tree::new("README.md\ndocs/migration-plan.md\nsrc/main.rs\nsrc/lib/mod.rs\n");
        let directory = tempfile::tempdir().expect("temporary directory");
        let mirror = Mirror::at(directory.path().join("github-mirror.db"));
        mirror.write("north/issues/open", &json!([{"number": 9}]), None, 1.0).expect("write open issues");
        mirror.write("north/issues/closed", &json!({"numbers": [4, 5]}), None, 1.0).expect("write closed issues");
        let state = |repository: &str, number: u64| recorded_issue_state(&mirror, repository, number);

        let closed = findings("docs/migration-plan.md", "Owners: North#4, #5\n\nMove `src/main.rs`.\n", &tree, "north", false, &state);
        assert_eq!(closed.len(), 1, "{closed:?}");
        assert!(closed[0].contains("every owner (north#4, north#5) is closed"), "{closed:?}");
        assert!(findings("docs/migration-plan.md", "Owners: north#4, north#9\n", &tree, "north", true, &state).is_empty());
        assert!(findings("docs/migration-plan.md", "Owners: wisp#7\n", &tree, "north", true, &state).is_empty());

        let text = "Move `src/main.rs`, `src/lib/`, `src/gone.rs:12` and `owner/repo`.\nCreates: `src/new.rs`\n";
        assert!(findings("docs/port-plan.md", text, &tree, "north", false, &state).is_empty());
        let touched = findings("docs/port-plan.md", text, &tree, "north", true, &state);
        assert_eq!(touched.len(), 2, "{touched:?}");
        assert!(touched[0].contains("names no owning issue"));
        assert!(touched[1].contains("`src/gone.rs`"));

        assert!(is_plan_document("docs/rationale/migration-order.md"));
        assert!(is_plan_document("docs/Porting.md"));
        assert!(!is_plan_document("docs/architecture.md"));
        assert!(!is_plan_document("docs/features/04-x/0001-plan.md"));
    }
}
