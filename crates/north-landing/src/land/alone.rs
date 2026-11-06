use std::fs;
use std::path::Path;

use super::queue;

const REQUEST: &str = "check-alone";
const ASKED: &str = "check-alone-asked";

pub fn request(queue: &Path, now_micros: u64, gap_micros: u64) -> Result<bool, String> {
    let asked = fs::read_to_string(queue.join(ASKED)).ok().and_then(|text| text.trim().parse::<u64>().ok());
    if asked.is_some_and(|at| now_micros.saturating_sub(at) < gap_micros) {
        return Ok(false);
    }
    for (name, text) in [(REQUEST, String::new()), (ASKED, now_micros.to_string())] {
        queue::write_atomic(&queue.join(name), &text).map_err(|error| format!("cannot write {}: {error}", queue.join(name).display()))?;
    }
    Ok(true)
}

pub fn take(queue: &Path) -> bool {
    fs::remove_file(queue.join(REQUEST)).is_ok()
}
