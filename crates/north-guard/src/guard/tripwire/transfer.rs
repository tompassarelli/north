use super::publish::Request;
use super::{MUTATION, State, redirect, strip};

const DATA_FLAGS: &[&str] = &[
    "-d",
    "--data",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "--data-ascii",
];

pub(super) fn secret(s: &str) -> bool {
    let p = s.trim_end_matches('/');
    p.contains(".ssh/")
        || p.ends_with("/.ssh")
        || p.contains(".aws/")
        || p.ends_with("/.aws")
        || p.contains("_SECRET")
        || p.ends_with(".pem")
        || [
            "id_rsa",
            "id_ed25519",
            "id_ecdsa",
            ".config/sops",
            "/run/secrets",
        ]
        .iter()
        .any(|s| p.contains(s))
        || p.ends_with(".age")
}

fn http_option<'s>(prev: &str, s: &'s str, request: &mut Request<'s>) {
    match prev {
        "-X" | "--request" | "--method" => request.method = s,
        "-d" | "--data" | "--data-binary" | "--data-raw" | "--data-urlencode" | "--data-ascii"
        | "--json" | "--post-data" | "--body-data" | "-F" | "--form" => {
            request.body = true;
            request.mutation |= MUTATION.is_match(s);
        }
        _ => {}
    }
}

fn inline_data(s: &str) -> bool {
    (s.starts_with("-d") && s.len() > 2)
        || s.starts_with("--data=")
        || (s.starts_with("--data-") && s.contains('='))
        || ["--json=", "--post-data=", "--body-data=", "--form="]
            .iter()
            .any(|p| s.starts_with(p))
        || (s.starts_with("-F") && s.len() > 2)
}

fn http_word<'s>(s: &'s str, request: &mut Request<'s>) {
    if let Some(m) = s
        .strip_prefix("-X")
        .filter(|s| !s.is_empty())
        .or_else(|| s.strip_prefix("--request="))
        .or_else(|| s.strip_prefix("--method="))
    {
        request.method = m;
    } else if inline_data(s) {
        request.body = true;
        request.mutation |= MUTATION.is_match(s);
    } else if s.starts_with("-T") || s == "--upload-file" || s.starts_with("--upload-file=") {
        request.body = true;
    } else if s.starts_with("http://") || s.starts_with("https://") {
        request.target = s;
    }
}

fn file_flag(s: &str) -> bool {
    s.starts_with("-T")
        || s == "--upload-file"
        || s.starts_with("--upload-file=")
        || s == "--post-file"
        || s.starts_with("--post-file=")
        || s == "--body-file"
        || s.starts_with("--body-file=")
}

fn inline_upload(s: &str) -> bool {
    [
        "-d@",
        "--data=@",
        "--data-binary=@",
        "--data-raw=@",
        "--data-urlencode=@",
        "--form=@",
    ]
    .iter()
    .any(|p| s.starts_with(p))
        || (s.starts_with("-F") && s.contains("=@"))
}

fn uploads(prev: &str, s: &str) -> bool {
    (DATA_FLAGS.contains(&prev) && s.starts_with('@'))
        || (["-F", "--form"].contains(&prev) && (s.starts_with('@') || s.contains("=@")))
        || file_flag(s)
        || inline_upload(s)
}

fn api_path(url: &str) -> Option<&str> {
    (url.contains("api.github.com/") || url.contains("/api/v3/")).then(|| {
        url.split_once("://")
            .and_then(|(_, s)| s.split_once('/'))
            .map(|(_, p)| p)
            .unwrap_or(url)
    })
}

fn remote_host(dest: &str) -> Option<&str> {
    let host = if let Some(p) = dest.strip_prefix("rsync://") {
        p.split('/').next().unwrap_or("")
    } else if dest.starts_with('/') || dest.starts_with("./") || dest.starts_with("../") {
        return None;
    } else {
        dest.split_once(':')?.0
    };
    let host = host.split_once('@').map(|(_, h)| h).unwrap_or(host);
    let local = ["", "localhost", "127.0.0.1", "::1"].contains(&host);
    let plain = host
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
    (!local && plain).then_some(host)
}

fn operands<'s>(verb: &str, args: &'s [String]) -> Vec<&'s str> {
    let flags = if verb == "scp" {
        &["-i", "--identity", "-o", "-P", "-F", "-J", "-S"][..]
    } else {
        &["-e", "-f", "-B", "--rsh"][..]
    };
    let mut found = Vec::new();
    let mut skip = false;
    for t in args {
        if skip {
            skip = false;
            continue;
        }
        if let Some(next) = redirect(t) {
            skip = next;
            continue;
        }
        if t.starts_with('-') {
            skip = flags.contains(&t.as_str());
            continue;
        }
        found.push(strip(t));
    }
    found
}

impl State<'_> {
    pub(super) fn exfil(&self, verb: &str) -> Result<(), String> {
        if self.secret_hit && !self.localhost {
            Err(format!(
                "secret path + network verb '{verb}' in one command — credential exfil surface (local reads alone are fine)"
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn http(&self, verb: &str, args: &[String]) -> Result<(), String> {
        self.exfil(verb)?;
        let mut request = Request::default();
        let mut prev = "";
        for t in args {
            let s = strip(t);
            http_option(prev, s, &mut request);
            http_word(s, &mut request);
            request.upload |= uploads(prev, s);
            prev = s;
        }
        if let Some(path) = api_path(request.target) {
            self.git_write(verb, request.method(), path, request.mutation)?;
        }
        if request.upload && !self.localhost {
            return Err(format!(
                "{verb} file upload to non-localhost — outbound exfil surface"
            ));
        }
        Ok(())
    }
    pub(super) fn copy(&self, verb: &str, args: &[String]) -> Result<(), String> {
        let mut sources = operands(verb, args);
        if sources.len() < 2 {
            return Ok(());
        }
        let dest = sources.pop().unwrap();
        let Some(host) = remote_host(dest) else {
            return Ok(());
        };
        for s in sources {
            if secret(s) {
                return Err(format!(
                    "{verb} of secret path '{s}' to remote host '{host}' — credential exfil surface (non-secret uploads are allowed)"
                ));
            }
        }
        Ok(())
    }
}
