use std::path::{Path, PathBuf};

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, params_from_iter};
use sha2::{Digest, Sha256};

use super::declared::Declarations;

type Decryptor = cbc::Decryptor<aes::Aes128>;
type Encryptor = cbc::Encryptor<aes::Aes128>;

const BASIC_STORE_PASSWORD: &[u8] = b"peanuts";
const HOST_HASH_SINCE: i64 = 24;
const HASH_LEN: usize = 32;

fn key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1, &mut key);
    key
}

fn database(profile: &Path) -> Option<PathBuf> {
    ["Default/Network/Cookies", "Default/Cookies"].iter().map(|relative| profile.join(relative)).find(|path| path.is_file())
}

fn version(connection: &Connection) -> i64 {
    connection.query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| row.get::<_, String>(0)).ok().and_then(|text| text.parse().ok()).unwrap_or(0)
}

fn decrypt(blob: &[u8], host: &str, hashed: bool, keyring: &mut impl FnMut() -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let (password, body) = match blob.split_at_checked(3) {
        Some((b"v10", body)) => (BASIC_STORE_PASSWORD.to_vec(), body),
        Some((b"v11", body)) => (keyring()?, body),
        _ => return Err("a cookie value has an unknown encryption prefix".into()),
    };
    let plain = Decryptor::new(&key(&password).into(), &[b' '; 16].into()).decrypt_padded_vec_mut::<Pkcs7>(body).map_err(|_| "a cookie value did not decrypt with the profile's key".to_owned())?;
    match hashed {
        true if plain.len() >= HASH_LEN && plain[..HASH_LEN] == Sha256::digest(host.as_bytes())[..] => Ok(plain[HASH_LEN..].to_vec()),
        true => Err("a cookie value does not carry its host hash".into()),
        false => Ok(plain),
    }
}

fn encrypt(plain: &[u8], host: &str, hashed: bool) -> Vec<u8> {
    let mut data = if hashed { Sha256::digest(host.as_bytes()).to_vec() } else { Vec::new() };
    data.extend_from_slice(plain);
    let mut blob = b"v10".to_vec();
    blob.extend(Encryptor::new(&key(BASIC_STORE_PASSWORD).into(), &[b' '; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(&data));
    blob
}

fn columns(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare("SELECT name FROM pragma_table_info('cookies')").map_err(|error| error.to_string())?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).map_err(|error| error.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|error| error.to_string())
}

pub struct Import<'a> {
    pub site: &'a str,
    pub from: &'a Path,
    pub to: &'a Path,
}

/// Copies the site's cookies from one Chrome profile into another, re-encrypted under the target's basic password store; the source is only read from a copy and no value is returned or printed.
pub fn import(request: &Import, declarations: &Declarations, mut keyring: impl FnMut() -> Result<Vec<u8>, String>) -> Result<usize, String> {
    if !declarations.browsers.iter().any(|browser| browser.host == request.site) {
        let known: Vec<&str> = declarations.browsers.iter().map(|browser| browser.host.as_str()).collect();
        return Err(format!("{} is not a declared browser site (declared: {})", request.site, known.join(", ")));
    }
    let source_path = database(request.from).ok_or_else(|| format!("no cookie store under {}", request.from.display()))?;
    let target_path = database(request.to).ok_or_else(|| format!("no cookie store under {}", request.to.display()))?;
    let copy = tempfile::tempdir().map_err(|error| error.to_string())?;
    let local = copy.path().join("Cookies");
    std::fs::copy(&source_path, &local).map_err(|error| format!("cannot copy {}: {error}", source_path.display()))?;
    let source = Connection::open_with_flags(&local, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    let mut target = Connection::open(&target_path).map_err(|error| error.to_string())?;
    target.busy_timeout(std::time::Duration::ZERO).map_err(|error| error.to_string())?;
    let (source_hashed, target_hashed) = (version(&source) >= HOST_HASH_SINCE, version(&target) >= HOST_HASH_SINCE);
    let source_columns = columns(&source)?;
    let shared: Vec<String> = columns(&target)?.into_iter().filter(|name| source_columns.contains(name)).collect();
    let host_at = shared.iter().position(|name| name == "host_key").ok_or("the cookie store has no host_key")?;
    let value_at = shared.iter().position(|name| name == "encrypted_value").ok_or("the cookie store has no encrypted_value")?;
    let list = shared.join(", ");
    let select = format!("SELECT {list} FROM cookies WHERE host_key = ?1 OR host_key = '.' || ?1 OR host_key LIKE '%.' || ?1");
    let mut statement = source.prepare(&select).map_err(|error| error.to_string())?;
    let rows: Vec<Vec<Value>> = statement
        .query_map([request.site], |row| (0..shared.len()).map(|index| row.get::<_, Value>(index)).collect())
        .map_err(|error| error.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let mut cached: Option<Vec<u8>> = None;
    let mut fetch = || match &cached {
        Some(password) => Ok(password.clone()),
        None => keyring().inspect(|password| cached = Some(password.clone())),
    };
    let mut imported = Vec::new();
    for mut row in rows {
        let Value::Text(host) = row[host_at].clone() else { continue };
        if let Value::Blob(blob) = row[value_at].clone() && !blob.is_empty() {
            let plain = decrypt(&blob, &host, source_hashed, &mut fetch)?;
            row[value_at] = Value::Blob(encrypt(&plain, &host, target_hashed));
        }
        imported.push(row);
    }
    let transaction = target.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| format!("cannot write {}: {error}; stop automation-chrome first", target_path.display()))?;
    transaction
        .execute("DELETE FROM cookies WHERE host_key = ?1 OR host_key = '.' || ?1 OR host_key LIKE '%.' || ?1", [request.site])
        .map_err(|error| error.to_string())?;
    let marks = vec!["?"; shared.len()].join(", ");
    for row in &imported {
        transaction.execute(&format!("INSERT OR REPLACE INTO cookies ({list}) VALUES ({marks})"), params_from_iter(row.iter())).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(imported.len())
}
