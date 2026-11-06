#[derive(Debug, Eq, PartialEq)]
pub enum Snapshot {
    Absent,
    Object(String),
}

impl Snapshot {
    pub fn render(&self) -> String {
        match self {
            Self::Absent => "absent".to_owned(),
            Self::Object(oid) => format!("object:{oid}"),
        }
    }
}

pub fn snapshot(observed: &str, destination_ref: &str) -> Result<Snapshot, String> {
    if observed.is_empty() {
        return Ok(Snapshot::Absent);
    }
    let refused = |what: &str| format!("safe-push: origin destination {destination_ref} is {what} — refusing\n");
    let mut found = Vec::new();
    for line in observed.split('\n') {
        let fields: Vec<&str> = line.split('\t').filter(|field| !field.is_empty()).collect();
        let value = fields.first().copied().unwrap_or("");
        if value.starts_with("ref: ") {
            return Err(refused("symbolic"));
        }
        if !object_id(value) || fields.get(1) != Some(&destination_ref) || fields.len() > 2 {
            return Err(refused("malformed or ambiguous"));
        }
        found.push(value);
    }
    match found.as_slice() {
        [oid] => Ok(Snapshot::Object((*oid).to_owned())),
        _ => Err(refused("malformed or ambiguous")),
    }
}

fn object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
