use rustix::fs::OFlags;

use crate::classify::Role;

const AT_FDCWD: i32 = -100;

pub enum RoleSource {
    Fixed(Role),
    Flags(u64),
    How(u64),
}

pub struct Target {
    pub dirfd: i32,
    pub pointer: u64,
    pub role: RoleSource,
    pub fd_flags: Option<u64>,
}

pub enum Plan {
    Paths(Vec<Target>),
    Listing(i32),
    Ambient,
}

pub fn plan(name: &str, args: &[u64; 6]) -> Plan {
    let dirfd = |index: usize| args[index] as u32 as i32;
    let one = |dirfd: i32, pointer: u64, role: RoleSource| Plan::Paths(vec![Target { dirfd, pointer, role, fd_flags: None }]);
    let fixed = |role: Role| RoleSource::Fixed(role);
    match name {
        "open" => one(AT_FDCWD, args[0], RoleSource::Flags(args[1])),
        "openat" => one(dirfd(0), args[1], RoleSource::Flags(args[2])),
        "openat2" => one(dirfd(0), args[1], RoleSource::How(args[2])),
        "creat" => one(AT_FDCWD, args[0], fixed(Role::Open { write: true, create: true })),
        "execve" => one(AT_FDCWD, args[0], fixed(Role::Exec)),
        "execveat" => one(dirfd(0), args[1], fixed(Role::Exec)),
        "stat" | "lstat" | "access" | "readlink" => one(AT_FDCWD, args[0], fixed(Role::Probe)),
        "faccessat" | "readlinkat" => one(dirfd(0), args[1], fixed(Role::Probe)),
        "statx" => Plan::Paths(vec![Target { dirfd: dirfd(0), pointer: args[1], role: fixed(Role::Probe), fd_flags: Some(args[2]) }]),
        "newfstatat" | "faccessat2" => Plan::Paths(vec![Target { dirfd: dirfd(0), pointer: args[1], role: fixed(Role::Probe), fd_flags: Some(args[3]) }]),
        "getdents" | "getdents64" => Plan::Listing(dirfd(0)),
        "inotify_add_watch" => one(AT_FDCWD, args[1], fixed(Role::Watch)),
        "mkdir" => one(AT_FDCWD, args[0], fixed(Role::MakeDir)),
        "mkdirat" => one(dirfd(0), args[1], fixed(Role::MakeDir)),
        "symlink" | "link" => one(AT_FDCWD, args[1], fixed(Role::Place)),
        "symlinkat" => one(dirfd(1), args[2], fixed(Role::Place)),
        "linkat" => one(dirfd(2), args[3], fixed(Role::Place)),
        "rename" => Plan::Paths(vec![Target { dirfd: AT_FDCWD, pointer: args[0], role: fixed(Role::Write), fd_flags: None }, Target { dirfd: AT_FDCWD, pointer: args[1], role: fixed(Role::Place), fd_flags: None }]),
        "renameat" | "renameat2" => Plan::Paths(vec![Target { dirfd: dirfd(0), pointer: args[1], role: fixed(Role::Write), fd_flags: None }, Target { dirfd: dirfd(2), pointer: args[3], role: fixed(Role::Place), fd_flags: None }]),
        "unlink" | "rmdir" | "truncate" => one(AT_FDCWD, args[0], fixed(Role::Write)),
        "unlinkat" => one(dirfd(0), args[1], fixed(Role::Write)),
        _ => Plan::Ambient,
    }
}

pub fn open_role(flags: u64) -> Role {
    let flags = OFlags::from_bits_retain(flags as u32);
    let write = flags.intersects(OFlags::WRONLY | OFlags::RDWR | OFlags::CREATE | OFlags::TRUNC);
    Role::Open { write: write && !flags.contains(OFlags::PATH), create: flags.contains(OFlags::CREATE) && !flags.contains(OFlags::PATH) }
}
