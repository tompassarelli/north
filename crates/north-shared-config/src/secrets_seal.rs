//! Which files under secrets/ may land (north#1626): a sops-encrypted YAML file, an OpenSSH public key in a `.pub` file, or an empty placeholder.
//! The landing refusals and the coordinator's landing guard both judge a secrets/ file through [`sealed`], so they cannot disagree.

pub const DIRECTORY: &str = "secrets/";
const PUBLIC_KEY_PREFIX: &str = "ssh-ed25519 ";
const SOPS_YAML_HEADER: &str = "sops:";
const SOPS_MAC_PREFIX: &str = "mac: ENC[AES256_GCM,";

/// Whether `file` lies under secrets/, the only directory whose files this gate judges.
pub fn governs(file: &str) -> bool {
    file.starts_with(DIRECTORY)
}

/// Whether a file under secrets/ with these `content` bytes may land: a `.yaml` or `.yml` file whose top-level `sops:` block
/// carries an encrypted `mac`, a `.pub` file holding one OpenSSH ed25519 public key, or an empty file, which holds no secret. Any other file, including a plaintext template, is not sealed.
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
    text.lines()
        .skip_while(|line| *line != SOPS_YAML_HEADER)
        .skip(1)
        .take_while(|line| line.starts_with(' ') || line.is_empty())
        .any(|line| line.trim_start().starts_with(SOPS_MAC_PREFIX))
}
