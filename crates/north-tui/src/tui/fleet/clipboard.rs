use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

use base64::Engine as _;

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipImage {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

pub struct Clipboard {
    copy: &'static str,
    paste: &'static str,
}

impl Clipboard {
    pub fn system() -> Self {
        Self::with_tools("wl-copy", "wl-paste")
    }

    pub fn with_tools(copy: &'static str, paste: &'static str) -> Self {
        Self { copy, paste }
    }

    pub fn image(&self) -> NorthResult<Option<ClipImage>> {
        let listed = self.wl_paste(&["--list-types"])?;
        let Some(media_type) = image_type(&String::from_utf8_lossy(&listed)).map(str::to_owned) else { return Ok(None) };
        let bytes = self.wl_paste(&["--type", &media_type])?;
        Ok(attachments::extension(&media_type).filter(|_| !bytes.is_empty()).map(|extension| ClipImage { extension, bytes }))
    }

    pub fn put_text(&self, text: &str) -> NorthResult<()> {
        let mut child = Command::new(self.copy).args(["--type", "text/plain;charset=utf-8"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|error| NorthError::Usage(format!("cannot run {}: {error}", self.copy)))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(NorthError::Usage(format!("{} failed: {status}", self.copy)));
        }
        Ok(())
    }

    fn wl_paste(&self, arguments: &[&str]) -> NorthResult<Vec<u8>> {
        let output = Command::new(self.paste).args(arguments).output().map_err(|error| match error.kind() {
            ErrorKind::NotFound => NorthError::Usage(format!("pasting an image needs {} (from wl-clipboard), which is not installed here; a terminal cannot paste an image", self.paste)),
            _ => NorthError::Usage(format!("cannot run {}: {error}", self.paste)),
        })?;
        if !output.status.success() {
            return Err(NorthError::Usage(format!("{} {} failed: {}", self.paste, arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())));
        }
        Ok(output.stdout)
    }
}

pub fn osc52_copy(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))
}

pub fn image_type(listed: &str) -> Option<&str> {
    let mut types = listed.lines().map(str::trim).filter(|line| attachments::extension(line).is_some());
    let first = types.next()?;
    Some(if first == "image/png" { first } else { types.find(|line| *line == "image/png").unwrap_or(first) })
}
