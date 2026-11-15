use std::path::PathBuf;

use super::super::Context;

#[test]
fn the_sealed_view_covers_the_temporary_directory_the_session_host_names() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let launcher = tempfile::tempdir_in("/tmp").unwrap();
    let launcher = launcher.path().canonicalize().unwrap();
    let (own, other) = (launcher.join("north-opencode-own"), launcher.join("north-opencode-other"));
    for directory in [own.join("tmp"), other.join("data"), home.join("code/north/main")] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let context = Context { home: home.clone(), cwd: home.join("code/north/main"), uid: 0, user_runtime: home.join("run"), tmpdir: Some(own.join("tmp")), cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None, session: None, agent_socket: None, offload_slot: None, land_check: false };
    let hidden: Vec<PathBuf> = super::hidden(&context, Some(&own), &[]).iter().map(|path| path.canonicalize().unwrap_or_else(|_| path.clone())).collect();
    assert!(hidden.iter().any(|path| other.join("data").starts_with(path)), "another session's scratch beside this one in the launcher's temporary directory {} stays readable: {hidden:?}", launcher.display());
    assert!(!hidden.iter().any(|path| own.join("tmp").starts_with(path) || path.starts_with(&own)), "the session's own scratch {} is hidden: {hidden:?}", own.display());
    let logins = super::super::readable::unreadable(&context, super::super::readable::Reader::Worker);
    for named in [home.as_path(), std::path::Path::new("/"), std::path::Path::new("/tmp")] {
        let hidden = super::hidden(&context, Some(named), &[]);
        let lifted: Vec<&PathBuf> = logins.iter().filter(|login| !hidden.contains(login)).collect();
        assert!(lifted.is_empty(), "a scratch the caller names as {} unhides {lifted:?}", named.display());
    }
}
