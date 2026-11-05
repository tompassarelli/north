use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipImage {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

pub struct Wayland;

impl Wayland {
    pub fn image(&self) -> NorthResult<Option<ClipImage>> {
        let listed = wl_paste(&["--list-types"])?;
        let Some(media_type) = image_type(&String::from_utf8_lossy(&listed)).map(str::to_owned) else { return Ok(None) };
        let bytes = wl_paste(&["--type", &media_type])?;
        Ok(attachments::extension(&media_type).filter(|_| !bytes.is_empty()).map(|extension| ClipImage { extension, bytes }))
    }

    pub fn put_text(&self, text: &str) -> NorthResult<()> {
        let mut child = Command::new("wl-copy").args(["--type", "text/plain;charset=utf-8"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|error| NorthError::Usage(format!("cannot run wl-copy: {error}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(NorthError::Usage(format!("wl-copy failed: {status}")));
        }
        Ok(())
    }
}

pub fn image_type(listed: &str) -> Option<&str> {
    let mut types = listed.lines().map(str::trim).filter(|line| attachments::extension(line).is_some());
    let first = types.next()?;
    Some(if first == "image/png" { first } else { types.find(|line| *line == "image/png").unwrap_or(first) })
}

fn wl_paste(arguments: &[&str]) -> NorthResult<Vec<u8>> {
    let output = Command::new("wl-paste").args(arguments).output().map_err(|error| NorthError::Usage(format!("cannot run wl-paste: {error}")))?;
    if !output.status.success() {
        return Err(NorthError::Usage(format!("wl-paste {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(output.stdout)
}
