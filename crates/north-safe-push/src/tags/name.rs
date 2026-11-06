pub fn release_tag(tag: &str) -> bool {
    tag.starts_with('v')
}

pub fn release_name(version: &str) -> String {
    format!("v{}", version.strip_prefix('v').unwrap_or(version))
}
