use std::path::{Path, PathBuf};

use crate::paths::{PROC, normalize};

pub enum Subject {
    Call(&'static str),
    OwnProc(&'static str),
    FileBeneath { root: &'static str, name: &'static str },
    AbsentLibrary,
}

pub struct NotInput {
    pub subject: Subject,
    #[allow(dead_code, reason = "each reason documents why its subject is not an input; the const is the list a reader audits")]
    pub reason: &'static str,
}

pub const NOT_INPUTS: &[NotInput] = &[
    NotInput { subject: Subject::AbsentLibrary, reason: "the dynamic loader probes every LD_LIBRARY_PATH directory for libraries the build did not put there, and the build key fixes what it did" },
    NotInput { subject: Subject::Call("sched_getaffinity"), reason: "libtest and std size their thread pools from host parallelism; test code that reads it stays caught by the ambient-test-input rule" },
    NotInput { subject: Subject::OwnProc("cgroup"), reason: "std's available_parallelism reads the cgroup path to find the cpu limit" },
    NotInput { subject: Subject::OwnProc("mountinfo"), reason: "std's available_parallelism reads the mounts to find the cgroup root" },
    NotInput { subject: Subject::FileBeneath { root: "/sys/fs/cgroup", name: "cpu.max" }, reason: "std's available_parallelism reads the cgroup cpu limit" },
    NotInput { subject: Subject::OwnProc("maps"), reason: "glibc pthread_getattr_np reads the memory map to find the main thread stack when std starts, so every Rust program reads it" },
    NotInput { subject: Subject::OwnProc("exe"), reason: "the link names the test binary, which the build key fixes" },
    NotInput { subject: Subject::OwnProc("cwd"), reason: "the link names the working directory, which the run fixes" },
    NotInput { subject: Subject::OwnProc("root"), reason: "the link names the process root, which the sandbox fixes" },
    NotInput { subject: Subject::OwnProc("fd"), reason: "each link names a file the process opened, recorded at its open" },
];

pub fn is_call(name: &str) -> bool {
    NOT_INPUTS.iter().any(|not_input| matches!(&not_input.subject, Subject::Call(call) if *call == name))
}

pub fn is_path(library_dirs: &[PathBuf], pid: u32, path: &Path, absent: bool) -> bool {
    NOT_INPUTS.iter().any(|not_input| match &not_input.subject {
        Subject::Call(_) => false,
        Subject::OwnProc(name) => own_proc(pid, path).is_some_and(|rest| rest.components().next().is_some_and(|first| first.as_os_str() == *name)),
        Subject::FileBeneath { root, name } => path.starts_with(root) && path.file_name().is_some_and(|file| file == *name),
        Subject::AbsentLibrary => absent && library_dirs.iter().any(|dir| path.strip_prefix(dir).is_ok_and(is_library_probe)),
    })
}

pub fn own_proc(pid: u32, path: &Path) -> Option<&Path> {
    let rest = path.strip_prefix(PROC).ok()?;
    let mut components = rest.components();
    let first = components.next()?.as_os_str();
    (first == "self" || first == "thread-self" || first.to_str().is_some_and(|name| name == pid.to_string())).then_some(components.as_path())
}

pub fn library_dirs(search_path: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    search_path.iter().flat_map(std::env::split_paths).filter(|dir| dir.is_absolute()).map(|dir| normalize(&dir)).collect()
}

fn is_library_probe(relative: &Path) -> bool {
    let hwcaps = relative.components().next().is_some_and(|first| first.as_os_str() == "glibc-hwcaps");
    hwcaps || relative.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("lib") && name.contains(".so"))
}
