use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const USAGE: &str = "usage: north stale-revert [--repo DIR] [--lane TIP] BASE HEAD";
const RECENT_SECONDS: i64 = 48 * 60 * 60;
const WIPE_FLOOR: usize = 3;
const BLAME_WORKERS: usize = 8;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Reverted {
    pub(crate) commit: String,
    pub(crate) subject: String,
    pub(crate) files: BTreeMap<String, usize>,
}

struct Window {
    commits: BTreeSet<String>,
    unseen: BTreeSet<String>,
    limit: Vec<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Hunk {
    start: usize,
    count: usize,
    added: BTreeSet<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct FileChange {
    path: String,
    hunks: Vec<Hunk>,
    added: BTreeSet<String>,
    removed: BTreeSet<String>,
    submodule: bool,
    deleted: bool,
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn commit_set(listing: &str) -> BTreeSet<String> {
    listing.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn recent_window(repo: &Path, base: &str, fork: &str) -> Result<Window, String> {
    let since_fork = commit_set(&git(repo, &["rev-list", &format!("{fork}..{base}")])?);
    let landed: i64 = git(repo, &["show", "-s", "--format=%ct", base])?.trim().parse().map_err(|_| format!("no commit time for {base}"))?;
    let since = format!("--since=@{}", landed - RECENT_SECONDS);
    let by_time = commit_set(&git(repo, &["rev-list", &since, base])?);
    Ok(if since_fork.len() >= by_time.len() {
        Window { commits: since_fork.clone(), unseen: since_fork, limit: vec![format!("{fork}..{base}")] }
    } else {
        Window { commits: by_time.union(&since_fork).cloned().collect(), unseen: since_fork, limit: vec![since, base.to_owned()] }
    })
}

fn patch_ids(repo: &Path, range: &str) -> Result<BTreeMap<String, String>, String> {
    let mut log = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "-p", "-U0", "--no-merges", "--no-color", "--no-ext-diff", range])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("git log -p {range}: {error}"))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["patch-id", "--stable"])
        .stdin(log.stdout.take().ok_or_else(|| format!("git log -p {range}: no output"))?)
        .output()
        .map_err(|error| format!("git patch-id: {error}"))?;
    let listed = log.wait().map_err(|error| format!("git log -p {range}: {error}"))?;
    if !listed.success() || !output.status.success() {
        return Err(format!("git log -p {range} | git patch-id failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(id, sha)| (sha.to_owned(), id.to_owned()))
        .collect())
}

fn carried(repo: &Path, fork: &str, base: &str, lane: &str) -> Result<BTreeSet<String>, String> {
    let lane_ids: BTreeSet<String> = patch_ids(repo, &format!("{fork}..{lane}"))?.into_values().collect();
    if lane_ids.is_empty() {
        return Ok(BTreeSet::new());
    }
    Ok(patch_ids(repo, &format!("{fork}..{base}"))?.into_iter().filter(|(_, id)| lane_ids.contains(id)).map(|(sha, _)| sha).collect())
}

fn named_reverts(repo: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    let trailers = git(repo, &["log", "--format=%(trailers:key=Reverts,valueonly)", &format!("{base}..{head}")])?;
    Ok(trailers
        .lines()
        .flat_map(|line| line.split([',', ' ']))
        .filter(|name| !name.is_empty())
        .filter_map(|name| git(repo, &["rev-parse", "--verify", "-q", &format!("{name}^{{commit}}")]).ok())
        .map(|sha| sha.trim().to_owned())
        .collect())
}

fn hunk(header: &str) -> Option<Hunk> {
    let old = header.strip_prefix("@@ -")?.split_whitespace().next()?;
    let (start, count) = old.split_once(',').unwrap_or((old, "1"));
    Some(Hunk { start: start.parse().ok()?, count: count.parse().ok()?, added: BTreeSet::new() })
}

fn changes(patch: &str) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    let mut in_header = false;
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            files.push(FileChange::default());
            in_header = true;
        } else if let (true, Some(file)) = (line.starts_with("@@ -"), files.last_mut()) {
            in_header = false;
            file.hunks.extend(hunk(line));
        } else if let (true, Some(path), Some(file)) = (in_header, line.strip_prefix("--- a/"), files.last_mut()) {
            file.path = path.strip_suffix('\t').unwrap_or(path).to_owned();
        } else if let (true, true, Some(file)) = (in_header, line == "+++ /dev/null", files.last_mut()) {
            file.deleted = true;
        } else if let (true, true, Some(file)) = (in_header, line.ends_with(" 160000"), files.last_mut()) {
            file.submodule = true;
        } else if let (false, Some(file)) = (in_header, files.last_mut()) {
            file.record(line);
        }
    }
    files
}

impl FileChange {
    fn record(&mut self, line: &str) {
        let (added, text) = match line.split_at_checked(1) {
            Some(("+", text)) => (true, text.trim()),
            Some(("-", text)) => (false, text.trim()),
            _ => return,
        };
        if !meaningful(text) {
            return;
        }
        if !added {
            self.removed.insert(text.to_owned());
            return;
        }
        self.added.insert(text.to_owned());
        if let Some(hunk) = self.hunks.last_mut() {
            hunk.added.insert(text.to_owned());
        }
    }

    fn removes_lines(&self) -> bool {
        self.hunks.iter().any(|hunk| hunk.count > 0)
    }

    fn hunk_at(&self, line: usize) -> Option<usize> {
        self.hunks.iter().position(|hunk| hunk.start <= line && line < hunk.start + hunk.count)
    }

    fn undoes(&self, earlier: &FileChange, blamed: &Blamed) -> bool {
        let put_back: BTreeSet<&String> = blamed.hunks.iter().flat_map(|&at| &self.hunks[at].added).collect();
        let restored = earlier.removed.iter().filter(|line| put_back.contains(line)).count();
        let restores = !earlier.removed.is_empty() && restored * 2 >= earlier.removed.len();
        let wipes = !self.deleted && earlier.added.len() >= WIPE_FLOOR && blamed.gone.len() * 5 >= earlier.added.len() * 4;
        restores || wipes
    }
}

fn meaningful(text: &str) -> bool {
    text.chars().any(|c| c.is_alphanumeric() || c == '_')
}

fn blame_header(line: &str) -> Option<(&str, usize)> {
    let mut fields = line.split(' ');
    let sha = fields.next()?;
    let numbers = fields.map(str::parse::<usize>).collect::<Result<Vec<_>, _>>().ok()?;
    let shaped = matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit());
    (shaped && matches!(numbers.len(), 2 | 3)).then(|| (sha, numbers[1]))
}

#[derive(Debug, Default)]
struct Blamed {
    removed: usize,
    gone: BTreeSet<String>,
    hunks: BTreeSet<usize>,
}

fn blamed_commits(repo: &Path, window: &Window, change: &FileChange, kept: &BTreeSet<&str>) -> Result<BTreeMap<String, Blamed>, String> {
    let mut args: Vec<String> = vec!["blame".into(), "--porcelain".into()];
    for hunk in change.hunks.iter().filter(|hunk| hunk.count > 0) {
        args.push(format!("-L{},+{}", hunk.start, hunk.count));
    }
    args.extend(window.limit.iter().cloned());
    args.extend(["--".to_owned(), change.path.clone()]);
    let porcelain = git(repo, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let mut counts: BTreeMap<String, Blamed> = BTreeMap::new();
    let mut current: Option<(&str, usize)> = None;
    for line in porcelain.lines() {
        if let Some(text) = line.strip_prefix('\t').map(str::trim) {
            let Some((sha, at)) = current.filter(|(sha, _)| window.commits.contains(*sha) && meaningful(text)) else { continue };
            let Some(index) = change.hunk_at(at) else { continue };
            let blamed = counts.entry(sha.to_owned()).or_default();
            blamed.removed += 1;
            blamed.hunks.insert(index);
            if change.hunks[index].added.is_empty() && !kept.contains(text) {
                blamed.gone.insert(text.to_owned());
            }
        } else if let Some(header) = blame_header(line) {
            current = Some(header);
        }
    }
    Ok(counts)
}

fn undone(repo: &Path, window: &Window, sha: &str, change: &FileChange, blamed: &Blamed) -> Result<bool, String> {
    if window.unseen.contains(sha) {
        return Ok(true);
    }
    let earlier = git(repo, &["show", "--format=", "-U0", "--no-color", "--no-renames", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/", sha, "--", &change.path])?;
    Ok(changes(&earlier).first().is_some_and(|earlier| change.undoes(earlier, blamed)))
}

fn generated(repo: &Path, head: &str, paths: &[&str]) -> Result<BTreeSet<String>, String> {
    if paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut args = vec!["check-attr", "-z", "--source", head, "linguist-generated", "--"];
    args.extend(paths);
    let listing = git(repo, &args)?;
    let fields: Vec<&str> = listing.split('\0').collect();
    Ok(fields.chunks_exact(3).filter(|entry| matches!(entry[2], "set" | "true")).map(|entry| entry[0].to_owned()).collect())
}

pub(crate) fn reverted(repo: &Path, base: &str, head: &str, lane: &str) -> Result<Vec<Reverted>, String> {
    let fork = git(repo, &["merge-base", base, lane])?.trim().to_owned();
    let mut window = recent_window(repo, base, &fork)?;
    let own = commit_set(&git(repo, &["rev-list", &format!("{fork}..{lane}")])?);
    let excused = named_reverts(repo, base, head)?;
    let landed_copies = if window.unseen.is_empty() { BTreeSet::new() } else { carried(repo, &fork, base, lane)? };
    window.commits.retain(|sha| !own.contains(sha) && !excused.contains(sha) && !landed_copies.contains(sha));
    if window.commits.is_empty() {
        return Ok(Vec::new());
    }
    let patch = git(repo, &["diff", "-U0", "--no-color", "--no-renames", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/", base, head])?;
    let all = changes(&patch);
    let kept: BTreeSet<&str> = all.iter().flat_map(|change| change.added.iter().map(String::as_str)).collect();
    let touched: Vec<&FileChange> = all.iter().filter(|change| !change.path.is_empty() && change.removes_lines() && !change.submodule).collect();
    let skipped = generated(repo, head, &touched.iter().map(|change| change.path.as_str()).collect::<Vec<_>>())?;
    let checked: Vec<&FileChange> = touched.into_iter().filter(|change| !skipped.contains(&change.path)).collect();
    let mut found: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (change, blamed) in checked.iter().zip(blame_all(repo, &window, &checked, &kept)?) {
        for (sha, blamed) in blamed {
            if undone(repo, &window, &sha, change, &blamed)? {
                found.entry(sha).or_default().insert(change.path.clone(), blamed.removed);
            }
        }
    }
    found
        .into_iter()
        .map(|(commit, files)| {
            let subject = git(repo, &["show", "-s", "--format=%s", &commit])?.trim().to_owned();
            Ok(Reverted { commit, subject, files })
        })
        .collect()
}

fn blame_all(repo: &Path, window: &Window, changes: &[&FileChange], kept: &BTreeSet<&str>) -> Result<Vec<BTreeMap<String, Blamed>>, String> {
    let workers = std::thread::available_parallelism().map_or(4, |count| count.get().min(BLAME_WORKERS));
    let chunk = changes.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = changes
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(|change| blamed_commits(repo, window, change, kept)).collect::<Result<Vec<_>, _>>()))
            .collect();
        let mut all = Vec::with_capacity(changes.len());
        for handle in handles {
            all.extend(handle.join().map_err(|_| "a blame worker panicked".to_owned())??);
        }
        Ok(all)
    })
}

pub(crate) fn refusal(found: &[Reverted]) -> String {
    let mut text = String::from("safe-push: ✗ this landing undoes recent commits on main (lines from a commit this lane never saw, or, per file, half the lines a commit removed restored or four fifths of the lines it added gone from a file that stays; deleting a whole file only undoes commits the lane never saw):\n");
    for reverted in found {
        let files: Vec<String> = reverted.files.iter().map(|(path, lines)| format!("{path} ({lines} lines)")).collect();
        text.push_str(&format!("  {} {}\n    {}\n", &reverted.commit[..9], reverted.subject, files.join(", ")));
    }
    text.push_str(
        "safe-push: rebase; don't copy whole files from another ref; squash only after rebasing. A deliberate revert names each commit in a 'Reverts: SHA' trailer. NOT pushed.",
    );
    text
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return 0;
    }
    let (repo, rest) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (Path::new(dir), rest),
        rest => (Path::new("."), rest),
    };
    let (lane, rest) = match rest {
        [flag, tip, rest @ ..] if flag == "--lane" => (Some(tip.as_str()), rest),
        rest => (None, rest),
    };
    let [base, head] = rest else {
        eprintln!("{USAGE}");
        return 64;
    };
    match reverted(repo, base, head, lane.unwrap_or(head)) {
        Ok(found) if found.is_empty() => 0,
        Ok(found) => {
            println!("{}", refusal(&found));
            1
        }
        Err(error) => {
            println!("safe-push: ✗ cannot check whether this landing reverts recent main: {error}; NOT pushed.");
            2
        }
    }
}

#[cfg(test)]
#[path = "stale_revert_tests.rs"]
mod tests;
