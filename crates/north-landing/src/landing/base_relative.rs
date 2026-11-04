use super::judge::{self, Quarantine, Verdict};
use super::land_cache::checkout_head;
use super::results::{Case, Env, Key, Results, Store};
use north_base::foundation::clock::Timestamp;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use time::OffsetDateTime;

pub struct Candidate {
    repository: String,
    base_tree: String,
    head_tree: String,
    env: Env,
    root: PathBuf,
}

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).stderr(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !text.is_empty()).then_some(text)
}

fn tree(revision: &str) -> Option<String> {
    git(&["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")])
}

fn here() -> Env {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let path = std::env::var("PATH").unwrap_or_default();
    let shell_closure = path.split(':').filter(|entry| entry.starts_with("/nix/store/")).collect::<Vec<_>>().join(":");
    let runner = match std::env::var("ImageOS") {
        Ok(image) if !image.is_empty() => format!("{image} {}", std::env::var("ImageVersion").unwrap_or_default()),
        _ => fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned(),
    };
    Env {
        south_revision: checkout_head(&home.join("code/south/main")),
        nixos_config_revision: checkout_head(&home.join("code/nixos-config/main")),
        shell_closure,
        runner,
    }
}

pub fn identity(test: &str) -> String {
    let path = Path::new(test);
    let hashed = path.extension().is_none().then(|| path.file_name()?.to_str()?.rsplit_once('-')).flatten();
    match hashed {
        Some((name, hash)) if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) => name.to_owned(),
        _ => test.to_owned(),
    }
}

fn short(tree: &str) -> &str {
    tree.get(..12).unwrap_or(tree)
}

impl Candidate {
    pub fn from_env() -> Option<Self> {
        let revision = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let (base, head) = (revision("SAFE_PUSH_LAND_BASE")?, revision("SAFE_PUSH_LAND_HEAD")?);
        let repository = git(&["rev-list", "--max-parents=0", &head])?.lines().min()?.to_owned();
        Some(Candidate { repository, base_tree: tree(&base)?, head_tree: tree(&head)?, env: here(), root: Store::default_root() })
    }

    fn key(&self, tree: &str, file: &str) -> Key {
        Key::new(tree, &format!("{} land-tests {file}", self.repository), &self.env)
    }

    fn quarantine_path(&self) -> PathBuf {
        self.root.join("quarantine").join(format!("{}.tsv", self.repository))
    }

    fn save(&self, quarantine: &Quarantine) -> io::Result<()> {
        let path = self.quarantine_path();
        let directory = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(directory)?;
        let mut pending = tempfile::NamedTempFile::new_in(directory)?;
        pending.write_all(quarantine.render().as_bytes())?;
        pending.persist(&path).map(|_| ()).map_err(|error| error.error)
    }

    pub fn judge(&self, cases: &[Case], out: &mut impl Write) -> u8 {
        let files = by_file(cases);
        let (base, candidate, unrecorded) = self.collect(&files, out);
        let now = OffsetDateTime::now_utc();
        let mut quarantine = fs::read_to_string(self.quarantine_path()).map(|text| Quarantine::parse(&text)).unwrap_or_default();
        quarantine.observe_base(&self.base_tree, &base, now);
        if let Err(error) = self.save(&quarantine) {
            let _ = writeln!(out, "land check: could not save the quarantine at {}: {error}", self.quarantine_path().display());
        }
        report_quarantined(&quarantine, &candidate, out);
        let selected: Vec<String> = files.into_keys().collect();
        match judge::judge(&base, &candidate, &quarantine, &selected, now) {
            Verdict::Land => 0,
            Verdict::Refuse { new } => {
                let named: Vec<String> = new.iter().map(|id| format!("{} {}", id.test, id.case)).collect();
                let _ = writeln!(out, "land check: {} new failing cases against base tree {}: {}", new.len(), short(&self.base_tree), named.join(", "));
                if !unrecorded.is_empty() {
                    let _ = writeln!(out, "land check: base tree {} has no recorded results for {}", short(&self.base_tree), unrecorded.join(", "));
                }
                1
            }
            Verdict::Hold { files } => {
                let _ = writeln!(out, "land check: held: {} has a case quarantined on main for over 24 h", files.join(", "));
                1
            }
        }
    }

    fn collect(&self, files: &BTreeMap<String, Vec<Case>>, out: &mut impl Write) -> (Results, Results, Vec<String>) {
        let store = Store::at(self.root.clone());
        let (mut base, mut candidate, mut unrecorded) = (Results::default(), Results::default(), Vec::new());
        for (file, ran) in files {
            match store.lookup(&self.key(&self.base_tree, file)) {
                Some(recorded) => base.extend(&recorded),
                None => unrecorded.push(file.clone()),
            }
            let head = self.key(&self.head_tree, file);
            if let Err(error) = store.record(&head, ran) {
                let _ = writeln!(out, "land check: could not record {file} under tree {}: {error}", short(&self.head_tree));
            }
            candidate.extend(&ran.iter().cloned().collect());
            if let Some(recorded) = store.lookup(&head) {
                candidate.extend(&recorded);
            }
        }
        (base, candidate, unrecorded)
    }
}

fn by_file(cases: &[Case]) -> BTreeMap<String, Vec<Case>> {
    let mut files: BTreeMap<String, Vec<Case>> = BTreeMap::new();
    for case in cases {
        let test = identity(&case.test);
        let name = if case.name == case.test { test.clone() } else { case.name.clone() };
        files.entry(test.clone()).or_default().push(Case { test, name, passed: case.passed });
    }
    files
}

fn report_quarantined(quarantine: &Quarantine, candidate: &Results, out: &mut impl Write) {
    for (id, entry) in quarantine.cases().filter(|(id, _)| candidate.get(&id.test, &id.case).is_some_and(|seen| seen.failed)) {
        let since = Timestamp::from_unix_seconds(entry.opened_at.unix_timestamp()).local_twelve_hour();
        let reason = match entry.reason {
            judge::Reason::BaseRed => "red",
            judge::Reason::Flaky => "flaky",
        };
        let _ = writeln!(out, "land check: quarantined {} {}: {reason} on main tree {} since {since}", id.test, id.case, short(&entry.tree));
    }
}
