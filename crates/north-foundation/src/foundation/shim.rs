use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn write_executable(path: &Path, content: impl AsRef<[u8]>) {
    let mut installer = Command::new("install")
        .args(["-m", "755", "/dev/stdin"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    installer.stdin.take().unwrap().write_all(content.as_ref()).unwrap();
    let status = installer.wait().unwrap();
    assert!(status.success(), "install {}: {status}", path.display());
}

pub fn write_script(path: &Path, body: &str) {
    write_executable(path, format!("#!/bin/sh\n{body}"));
}

pub fn copy_executable(source: impl AsRef<Path>, destination: impl AsRef<Path>) {
    let status = Command::new("install").args(["-m", "755"]).arg(source.as_ref()).arg(destination.as_ref()).status().unwrap();
    assert!(status.success(), "install {} {}: {status}", source.as_ref().display(), destination.as_ref().display());
}
