use std::path::Path;

use north_base::platform::git;

pub fn default_branch(repository: &Path) -> String {
    let origin_head = symbolic_ref(repository, "refs/remotes/origin/HEAD").unwrap_or_default();
    let default = origin_head.strip_prefix("refs/remotes/origin/").unwrap_or(&origin_head);
    if default.is_empty() { "main".to_owned() } else { default.to_owned() }
}

pub fn symbolic_ref(repository: &Path, name: &str) -> Option<String> {
    let output = git::run(repository, &["symbolic-ref", "--quiet", name]).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}
