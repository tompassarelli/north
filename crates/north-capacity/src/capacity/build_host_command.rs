//! The command a build host runs for an offloaded one.

use super::build_offload::TOOLCHAIN_PROGRAMS;

/// The command inside its confinement wrappers, or the command itself: a depbox wrapper (`depbox MODE --` or `north depbox MODE --`) and the worker-domain wrapper the sandbox broker puts around a `capacity host run` command (`north --worker-sandbox --program --`), which names this host's runtime path and would otherwise read as a laptop path a build host cannot run and as no compile at all.
pub fn unwrapped(command: &[String]) -> &[String] {
    let depbox = |word: &str| word.rsplit('/').next() == Some("depbox");
    let north = |word: &str| word.rsplit('/').next() == Some("north");
    let wrapped = |mode: &String, marker: &String, inner: &[String]| matches!(mode.as_str(), "run" | "fetch") && marker == "--" && !inner.is_empty();
    match command {
        [program, mode, marker, inner @ ..] if depbox(program) && wrapped(mode, marker, inner) => unwrapped(inner),
        [program, verb, mode, marker, inner @ ..] if north(program) && verb == "depbox" && wrapped(mode, marker, inner) => unwrapped(inner),
        [program, flag, mode, marker, inner @ ..] if north(program) && flag == "--worker-sandbox" && mode == "--program" && marker == "--" && !inner.is_empty() => unwrapped(inner),
        _ => command,
    }
}

/// The command as a build host runs it: a toolchain program the laptop named by its path (the gate runs the real program it found on PATH) named by its bare name, so the host finds its own copy in the slot's dev shell, and, unless it is a landing, inside `north depbox run` in place of any depbox or worker-domain wrapper the laptop gave, so its build scripts, proc macros and tests reach none of the build host's credentials and no network. A landing stays unwrapped: its checks enter depbox through land-shell, and its runner fetches crates, which a sandbox around it would refuse.
pub fn for_build_host(command: &[String], landing: bool) -> Vec<String> {
    let mut words = unwrapped(command).to_vec();
    if let Some(first) = words.first_mut() {
        let name = first.rsplit('/').next().unwrap_or_default();
        if first.starts_with('/') && TOOLCHAIN_PROGRAMS.contains(&name) {
            *first = name.to_owned();
        }
    }
    if landing || words.is_empty() {
        return words;
    }
    ["north", "depbox", "run", "--"].into_iter().map(str::to_owned).chain(words).collect()
}
