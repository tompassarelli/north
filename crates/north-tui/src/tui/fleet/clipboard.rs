use std::process::Command;

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipImage {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

pub trait Clipboard {
    fn image(&self) -> NorthResult<Option<ClipImage>>;
}

pub struct WlPaste;

impl Clipboard for WlPaste {
    fn image(&self) -> NorthResult<Option<ClipImage>> {
        let listed = wl_paste(&["--list-types"])?;
        let Some(media_type) = image_type(&String::from_utf8_lossy(&listed)).map(str::to_owned) else { return Ok(None) };
        let bytes = wl_paste(&["--type", &media_type])?;
        Ok(attachments::extension(&media_type).filter(|_| !bytes.is_empty()).map(|extension| ClipImage { extension, bytes }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_preferred_among_the_clipboard_image_types() {
        assert_eq!(image_type("text/plain\nimage/jpeg\nimage/png\n"), Some("image/png"));
        assert_eq!(image_type("text/plain\nimage/webp\n"), Some("image/webp"));
        assert_eq!(image_type("text/plain\ntext/html\n"), None);
    }
}
