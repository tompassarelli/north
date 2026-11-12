pub fn glob(pattern: &str, path: &str) -> bool {
    let pattern = pattern.split('/').collect::<Vec<_>>();
    let path = path.split('/').collect::<Vec<_>>();
    segments(&pattern, &path)
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| segments(rest, &path[skip..])),
        Some((first, rest)) => path.split_first().is_some_and(|(head, tail)| segment(first, head) && segments(rest, tail)),
    }
}

fn segment(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail),
    }
}
