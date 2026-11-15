use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use super::Contract;

static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*+]\s+").unwrap());
static SUBHEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{3,6}\s").unwrap());

pub(super) struct Block {
    pub(super) section: String,
    pub(super) digest: String,
    pub(super) text: String,
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(normalized(text).as_bytes()))
}

struct Reader {
    section: String,
    blocks: Vec<Block>,
    paragraph: Vec<String>,
    bullet: Vec<String>,
}

impl Reader {
    fn emit(section: &str, blocks: &mut Vec<Block>, parts: &mut Vec<String>) {
        if parts.is_empty() {
            return;
        }
        let text = normalized(&parts.iter().map(|part| part.trim()).collect::<Vec<_>>().join(" "));
        blocks.push(Block { section: section.to_owned(), digest: digest(&text), text });
        parts.clear();
    }

    fn flush(&mut self) {
        Self::emit(&self.section, &mut self.blocks, &mut self.paragraph);
        Self::emit(&self.section, &mut self.blocks, &mut self.bullet);
    }

    fn line(&mut self, line: &str) {
        if line.starts_with("# ") {
            self.flush();
        } else if let Some(title) = line.strip_prefix("## ") {
            self.flush();
            self.section = title.trim().to_owned();
        } else if SUBHEADING.is_match(line) || line.starts_with("    ") || line.trim().is_empty() {
            self.flush();
        } else if BULLET.is_match(line) {
            self.flush();
            self.bullet.push(line.to_owned());
        } else if self.bullet.is_empty() {
            self.paragraph.push(line.to_owned());
        } else {
            self.bullet.push(line.to_owned());
        }
    }
}

pub(super) fn markdown_blocks(text: &str) -> Vec<Block> {
    let mut reader = Reader {
        section: "preamble".into(),
        blocks: Vec::new(),
        paragraph: Vec::new(),
        bullet: Vec::new(),
    };
    let mut fenced = false;
    for line in text.lines() {
        if line.starts_with("```") {
            reader.flush();
            fenced = !fenced;
        } else if !fenced {
            reader.line(line);
        }
    }
    reader.flush();
    reader.blocks
}

pub(super) fn check(contract: &mut Contract, surfaces: &[(&str, &Path)]) {
    let mut seen: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (surface, path) in surfaces {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                contract.reject(format!(
                    "{surface} policy source is unreadable: {}: {error}",
                    path.display()
                ));
                continue;
            }
        };
        for block in markdown_blocks(&text) {
            if let Some((other_surface, other_section)) = seen.get(&block.digest) {
                contract.reject(format!(
                    "{surface} [{}] repeats the {other_surface} [{other_section}] block: {}",
                    block.section,
                    block.text.chars().take(80).collect::<String>()
                ));
                continue;
            }
            seen.insert(block.digest, (surface.to_string(), block.section));
        }
    }
}
