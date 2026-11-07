pub fn slug_of(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let mut parts = url.rsplit(['/', ':']);
    let (name, owner) = (parts.next()?, parts.next()?);
    (!name.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{name}"))
}
