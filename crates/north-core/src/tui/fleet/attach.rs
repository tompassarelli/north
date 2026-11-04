use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;

use super::clipboard::{ClipImage, Clipboard, WlPaste};

const KIB: usize = 1024;

pub struct Attachments {
    clipboard: Box<dyn Clipboard>,
    dir: PathBuf,
    items: Vec<(String, PathBuf)>,
}

impl Attachments {
    pub fn system() -> Self {
        let root = crate::fleet::registry::root_here().unwrap_or_else(|_| std::env::temp_dir().join("north-fleet"));
        Self::new(Box::new(WlPaste), root.join(attachments::DIR))
    }

    pub fn new(clipboard: Box<dyn Clipboard>, dir: PathBuf) -> Self {
        Self { clipboard, dir, items: Vec::new() }
    }

    pub fn attach(&mut self) -> NorthResult<String> {
        let image = self.clipboard.image()?.ok_or_else(|| NorthError::Usage("no image on the clipboard".into()))?;
        let path = self.store(&image)?;
        let chip = format!("[image {}: {} {}]", self.items.len() + 1, size(image.bytes.len()), image.extension);
        self.items.push((chip.clone(), path));
        Ok(chip)
    }

    pub fn expand(&mut self, text: &str) -> String {
        self.items.drain(..).fold(text.to_owned(), |text, (chip, path)| text.replace(&chip, &attachments::marker(&path)))
    }

    fn store(&self, image: &ClipImage) -> NorthResult<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let digest = Sha256::digest(&image.bytes);
        let name: String = digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect();
        let path = self.dir.join(format!("{name}.{}", image.extension));
        fs::write(&path, &image.bytes)?;
        Ok(path)
    }
}

fn size(bytes: usize) -> String {
    if bytes >= KIB * KIB {
        format!("{:.1} MB", bytes as f64 / (KIB * KIB) as f64)
    } else {
        format!("{} KB", bytes.div_ceil(KIB).max(1))
    }
}
