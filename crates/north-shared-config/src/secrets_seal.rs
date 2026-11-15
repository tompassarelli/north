//! Which files under secrets/ may land (north#1626): a sops-encrypted YAML file, an OpenSSH public key in a `.pub` file, or an empty placeholder.
//! The landing refusals and the coordinator's landing guard both judge a secrets/ file through [`sealed`], so they cannot disagree.

pub const DIRECTORY: &str = "secrets/";
const PUBLIC_KEY_PREFIX: &str = "ssh-ed25519 ";
const SOPS_YAML_HEADER: &str = "sops:";
const SOPS_MAC_PREFIX: &str = "mac: ENC[AES256_GCM,";
const SOPS_DEFAULT_UNENCRYPTED_SUFFIX: &str = "_unencrypted";
const SOPS_METADATA_INDENT: &str = "    ";
const ENCRYPTED: &str = "ENC[";

/// Whether `file` lies under secrets/, the only directory whose files this gate judges.
pub fn governs(file: &str) -> bool {
    file.starts_with(DIRECTORY)
}

/// Whether a file under secrets/ with these `content` bytes may land: a `.yaml` or `.yml` file whose top-level `sops:` block
/// carries an encrypted `mac` and whose every other leaf value is encrypted, as the block's own scope rule declares, a `.pub` file holding one OpenSSH ed25519 public key, or an empty file, which holds no secret. Any other file, including a plaintext template, is not sealed.
pub fn sealed(file: &str, content: &[u8]) -> bool {
    if content.is_empty() {
        return true;
    }
    let text = String::from_utf8_lossy(content);
    if file.ends_with(".pub") {
        return text.starts_with(PUBLIC_KEY_PREFIX) && !text.contains("PRIVATE");
    }
    if file.ends_with(".yaml") || file.ends_with(".yml") {
        return sops_yaml(&text);
    }
    false
}

fn sops_yaml(text: &str) -> bool {
    let (block, outside) = split_sops(text);
    let Some(rule) = ScopeRule::declared(&block) else { return false };
    block.iter().any(|line| line.trim_start().starts_with(SOPS_MAC_PREFIX)) && outside.iter().all(|line| !plaintext_leaf(line, &rule))
}

fn split_sops(text: &str) -> (Vec<&str>, Vec<&str>) {
    let lines: Vec<&str> = text.lines().collect();
    let Some(header) = lines.iter().position(|line| *line == SOPS_YAML_HEADER) else { return (Vec::new(), lines) };
    let body = header + 1;
    let end = lines[body..].iter().position(|line| !is_block_line(line)).map_or(lines.len(), |offset| body + offset);
    (lines[body..end].to_vec(), lines[..header].iter().chain(&lines[end..]).copied().collect())
}

fn is_block_line(line: &str) -> bool {
    line.starts_with(' ') || line.is_empty()
}

/// The value of a key written at the sops block's own indent, unquoted; `None` when the block does not write the key.
fn metadata<'a>(block: &[&'a str], key: &str) -> Option<&'a str> {
    block.iter().find_map(|line| {
        let rest = line.strip_prefix(SOPS_METADATA_INDENT)?.strip_prefix(key)?;
        rest.strip_prefix(':').map(|value| value.trim().trim_matches(['\'', '"']))
    })
}

/// Which keys of the file may hold plaintext, as the sops block declares: `unencrypted_suffix` (default `_unencrypted`) names the keys left plain, `encrypted_suffix` names the only keys sops encrypted.
enum ScopeRule<'a> {
    Unencrypted(&'a str),
    Encrypted(&'a str),
}

impl<'a> ScopeRule<'a> {
    fn declared(block: &[&'a str]) -> Option<Self> {
        if metadata(block, "unencrypted_regex").is_some() || metadata(block, "encrypted_regex").is_some() {
            return None;
        }
        let declared_suffix = |key| metadata(block, key).filter(|suffix| !suffix.is_empty());
        match (declared_suffix("unencrypted_suffix"), declared_suffix("encrypted_suffix")) {
            (Some(suffix), None) => Some(Self::Unencrypted(suffix)),
            (None, Some(suffix)) => Some(Self::Encrypted(suffix)),
            (None, None) => Some(Self::Unencrypted(SOPS_DEFAULT_UNENCRYPTED_SUFFIX)),
            (Some(_), Some(_)) => None,
        }
    }

    fn may_stay_plain(&self, key: Option<&str>) -> bool {
        match (self, key) {
            (_, None) => false,
            (Self::Unencrypted(suffix), Some(key)) => key.ends_with(suffix),
            (Self::Encrypted(suffix), Some(key)) => !key.ends_with(suffix),
        }
    }
}

fn plaintext_leaf(line: &str, rule: &ScopeRule) -> bool {
    let mut text = line.trim_start();
    while let Some(rest) = text.strip_prefix("- ") {
        text = rest.trim_start();
    }
    if text.is_empty() || text.starts_with('#') || matches!(text, "---" | "...") {
        return false;
    }
    let (key, value) = match text.split_once(": ") {
        Some((key, value)) => (Some(key), value),
        None if text.ends_with(':') => return false,
        None => (None, text),
    };
    let value = value.trim().trim_matches(['\'', '"']);
    if value.is_empty() || value.starts_with(ENCRYPTED) || value == "[]" || value == "{}" {
        return false;
    }
    !rule.may_stay_plain(key)
}
