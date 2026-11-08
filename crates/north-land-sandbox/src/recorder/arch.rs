use nix::unistd::Pid;

pub struct Call {
    pub number: i64,
    pub args: [u64; 6],
}

#[cfg(target_arch = "x86_64")]
pub const NUMBERS: &[(&str, i64)] = &[
    ("open", libc::SYS_open),
    ("openat", libc::SYS_openat),
    ("openat2", libc::SYS_openat2),
    ("creat", libc::SYS_creat),
    ("execve", libc::SYS_execve),
    ("execveat", libc::SYS_execveat),
    ("stat", libc::SYS_stat),
    ("lstat", libc::SYS_lstat),
    ("newfstatat", libc::SYS_newfstatat),
    ("statx", libc::SYS_statx),
    ("access", libc::SYS_access),
    ("faccessat", libc::SYS_faccessat),
    ("faccessat2", libc::SYS_faccessat2),
    ("readlink", libc::SYS_readlink),
    ("readlinkat", libc::SYS_readlinkat),
    ("getdents", libc::SYS_getdents),
    ("getdents64", libc::SYS_getdents64),
    ("inotify_add_watch", libc::SYS_inotify_add_watch),
    ("mkdir", libc::SYS_mkdir),
    ("mkdirat", libc::SYS_mkdirat),
    ("rename", libc::SYS_rename),
    ("renameat", libc::SYS_renameat),
    ("renameat2", libc::SYS_renameat2),
    ("link", libc::SYS_link),
    ("linkat", libc::SYS_linkat),
    ("symlink", libc::SYS_symlink),
    ("symlinkat", libc::SYS_symlinkat),
    ("unlink", libc::SYS_unlink),
    ("unlinkat", libc::SYS_unlinkat),
    ("rmdir", libc::SYS_rmdir),
    ("truncate", libc::SYS_truncate),
    ("statfs", libc::SYS_statfs),
    ("fstatfs", libc::SYS_fstatfs),
    ("sysinfo", libc::SYS_sysinfo),
    ("sched_getaffinity", libc::SYS_sched_getaffinity),
];

#[cfg(target_arch = "x86_64")]
pub const IO_URING_SETUP: i64 = libc::SYS_io_uring_setup;

#[cfg(target_arch = "x86_64")]
pub const TARGET: seccompiler::TargetArch = seccompiler::TargetArch::x86_64;

#[cfg(not(target_arch = "x86_64"))]
pub const NUMBERS: &[(&str, i64)] = &[];

#[cfg(target_arch = "x86_64")]
pub fn registers(pid: Pid) -> Result<Call, String> {
    let registers = nix::sys::ptrace::getregs(pid).map_err(|error| format!("registers of process {pid}: {error}"))?;
    Ok(Call { number: registers.orig_rax as i64, args: [registers.rdi, registers.rsi, registers.rdx, registers.r10, registers.r8, registers.r9] })
}

#[cfg(not(target_arch = "x86_64"))]
pub fn registers(_pid: Pid) -> Result<Call, String> {
    Err("the recorder runs on x86_64 only".into())
}

pub fn name(number: i64) -> Option<&'static str> {
    NUMBERS.iter().find(|(_, candidate)| *candidate == number).map(|(name, _)| *name)
}
