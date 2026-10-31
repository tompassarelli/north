use super::failure::{Failure, failed};
use super::layout::{self, Layout, TREE};
use std::fs;

pub(super) fn health(layout: &Layout) -> Result<(), Failure> {
    let executable = fs::canonicalize(std::env::current_exe()?)?;
    let current = fs::canonicalize(layout.current())?;
    if executable.parent() != Some(current.as_path()) {
        return Err(failed(1, format!("{} is not the current runtime {}", executable.display(), current.display())));
    }
    if !layout::has_programs(&current) || !current.join(TREE).is_dir() {
        return Err(failed(1, format!("{} lacks its programs or tree", current.display())));
    }
    println!("north runtime healthy: {}", current.file_name().unwrap_or_default().to_string_lossy());
    Ok(())
}
