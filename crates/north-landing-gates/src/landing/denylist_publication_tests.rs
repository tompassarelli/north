use north_base::test_inputs::git::git_in as git;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use super::scan_range;

const RULES: &str = "title = \"publication deny-list\"\n\
[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''zebra-quokka-7731'''\n\
[[rules]]\nid = \"fake-credit-lane\"\ndescription = \"fake credit-lane login\"\nregex = '''((?i:proton\\s+(?:credit|account|login)))'''\n\
[[rules]]\nid = \"fake-skill-host\"\ndescription = \"fake host named in a skill\"\nregex = '''quokka-host-42'''\npath = '''^harness/skills/'''\n\
[[rules]]\nid = \"private-anthropic-ids\"\ndescription = \"Anthropic environment or session ID\"\nregex = '''\\b((?:env|session)_01[A-Za-z0-9]{20,})'''\n";

struct Publication {
    scratch: tempfile::TempDir,
    repo: PathBuf,
    base: String,
}

impl Publication {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let repo = scratch.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(scratch.path().join("denylist.toml"), RULES).unwrap();
        let publication = Self { scratch, repo, base: String::new() };
        git(&publication.repo, &["init", "-q", "-b", "main"]);
        git(&publication.repo, &["config", "user.name", "safe-push-test"]);
        git(&publication.repo, &["config", "user.email", "safe-push-test@example.invalid"]);
        publication.commit("content.txt", "initial\n", "initial");
        publication.published()
    }

    fn write(&self, path: &str, content: &[u8]) {
        let file = self.repo.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }

    fn commit(&self, path: &str, content: &str, message: &str) {
        self.write(path, content.as_bytes());
        git(&self.repo, &["add", path]);
        git(&self.repo, &["commit", "-qm", message]);
    }

    fn published(mut self) -> Self {
        self.base = git(&self.repo, &["rev-parse", "HEAD"]);
        self
    }

    fn findings(&self, ref_name: Option<&str>) -> BTreeSet<String> {
        let config = self.scratch.path().join("denylist.toml");
        scan_range(&self.repo, &config, &self.base, "HEAD", ref_name).unwrap()
    }

    fn short(&self, revision: &str) -> String {
        git(&self.repo, &["rev-parse", revision])[..12].to_owned()
    }
}

fn set(findings: &[&str]) -> BTreeSet<String> {
    findings.iter().map(|finding| (*finding).to_owned()).collect()
}

fn rename_into_skills(content: &str) -> Publication {
    let publication = Publication::new();
    publication.commit("owner-notes.md", &format!("{content}\n"), "add owner notes");
    let publication = publication.published();
    std::fs::create_dir_all(publication.repo.join("harness/skills")).unwrap();
    git(&publication.repo, &["mv", "owner-notes.md", "harness/skills/owner-notes.md"]);
    git(&publication.repo, &["commit", "-qm", "move owner notes into skills"]);
    publication
}

#[test]
fn denylist_publication_scan_names_private_values_and_session_links_in_content_messages_and_refs() {
    let content = Publication::new();
    content.commit("identity.txt", "owner: zebra-quokka-7731\n", "add identity");
    assert_eq!(content.findings(None), set(&["fake-private-term in identity.txt"]));

    let clean = Publication::new();
    clean.commit("content.txt", "initial\nclean\n", "Record the owner");
    assert_eq!(clean.findings(None), BTreeSet::new());

    let message = Publication::new();
    message.commit("content.txt", "initial\nclean\n", "Record owner zebra-quokka-7731");
    assert_eq!(message.findings(None), set(&[&format!("fake-private-term in commit {}", message.short("HEAD"))]));

    let session = Publication::new();
    session.commit("content.txt", "initial\nclean\n", &format!("Record the run\n\nClaude-Session: https://claude.ai/code/{}", concat!("sess", "ion_01QwErTyUiOpAsDfGhJkLz")));
    assert_eq!(session.findings(None), set(&[&format!("private-anthropic-ids in commit {}", session.short("HEAD"))]));

    let author = Publication::new();
    author.write("content.txt", b"initial\nclean\n");
    git(&author.repo, &["-c", "author.name=zebra-quokka-7731", "commit", "-qam", "Record the owner"]);
    assert_eq!(author.findings(None), set(&[&format!("fake-private-term in commit {}", author.short("HEAD"))]));

    let branch = Publication::new();
    branch.commit("content.txt", "initial\nclean\n", "Record the owner");
    assert_eq!(branch.findings(Some("claude/zebra-quokka-7731")), set(&["fake-private-term in ref name"]));
    assert_eq!(branch.findings(Some("claude/record-owner")), BTreeSet::new());

    let ignored = Publication::new();
    ignored.write(".betterleaksignore", b"identity.txt:fake-private-term:1\n");
    ignored.commit("identity.txt", "owner: zebra-quokka-7731\n", "add identity");
    assert_eq!(ignored.findings(None), set(&["fake-private-term in identity.txt"]));

    let former = Publication::new();
    former.write(concat!(".git", "leaksignore"), b"identity.txt:fake-private-term:1\n");
    former.commit("identity.txt", "owner: zebra-quokka-7731\n", "add identity");
    assert_eq!(former.findings(None), set(&["fake-private-term in identity.txt"]));
}

#[test]
fn denylist_publication_scan_follows_renames_file_names_archives_and_skill_paths() {
    assert_eq!(rename_into_skills("owner: zebra-quokka-7731").findings(None), set(&["fake-private-term in harness/skills/owner-notes.md"]));
    assert_eq!(rename_into_skills("host: quokka-host-42").findings(None), set(&["fake-skill-host in harness/skills/owner-notes.md"]));
    assert_eq!(rename_into_skills("owner: the maintainer").findings(None), BTreeSet::new());

    let two_commits = Publication::new();
    two_commits.commit("owner-notes.md", "host: quokka-host-42\n", "add owner notes");
    std::fs::create_dir_all(two_commits.repo.join("harness/skills")).unwrap();
    git(&two_commits.repo, &["mv", "owner-notes.md", "harness/skills/owner-notes.md"]);
    git(&two_commits.repo, &["commit", "-qm", "move owner notes into skills"]);
    assert_eq!(two_commits.findings(None), set(&["fake-skill-host in harness/skills/owner-notes.md"]));

    let onto_skill = Publication::new();
    onto_skill.commit("harness/skills/owner-notes.md", "clean\n", "add clean skill notes");
    let onto_skill = onto_skill.published();
    onto_skill.commit("notes.md", "host: quokka-host-42\n", "add notes");
    git(&onto_skill.repo, &["rm", "-q", "harness/skills/owner-notes.md"]);
    git(&onto_skill.repo, &["commit", "-qm", "drop skill notes"]);
    std::fs::create_dir_all(onto_skill.repo.join("harness/skills")).unwrap();
    git(&onto_skill.repo, &["mv", "notes.md", "harness/skills/owner-notes.md"]);
    git(&onto_skill.repo, &["commit", "-qm", "move notes onto skill notes"]);
    assert_eq!(onto_skill.findings(None), set(&["fake-skill-host in harness/skills/owner-notes.md"]));

    let named = Publication::new();
    named.commit("zebra-quokka-7731.md", "clean\n", "add notes");
    assert_eq!(named.findings(None), set(&["fake-private-term in file names"]));
    git(&named.repo, &["mv", "zebra-quokka-7731.md", "notes.md"]);
    git(&named.repo, &["commit", "-qm", "rename notes"]);
    assert_eq!(named.findings(None), set(&["fake-private-term in file names"]));

    let unit = Publication::new();
    unit.commit("nix/north-fleet.target", "Description=owner zebra-quokka-7731\n", "add target unit");
    assert_eq!(unit.findings(None), set(&["fake-private-term in nix/north-fleet.target"]));

    let archive = Publication::new();
    let package = archive.scratch.path().join("package");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("package.json"), "{\"author\":\"zebra-quokka-7731\"}\n").unwrap();
    let tarball = archive.repo.join("pkg-1.0.0.tgz");
    let packed = Command::new("tar").arg("-czf").arg(&tarball).arg("-C").arg(archive.scratch.path()).arg("package").status().unwrap();
    assert!(packed.success());
    git(&archive.repo, &["add", "pkg-1.0.0.tgz"]);
    git(&archive.repo, &["commit", "-qm", "add package"]);
    let found = archive.findings(None);
    assert!(found.iter().any(|finding| finding.starts_with("fake-private-term in pkg-1.0.0.tgz")), "{found:?}");
}
