use north_base::platform::harness::{DEFAULTS, document};
use toml_edit::{DocumentMut, Value};

pub const TABLE: &str = "proxy_role";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Class {
    Launch,
    Send,
    Ask,
    Stop,
    Adopt,
    Gate,
    IssueBox,
    Status,
    PlacementLook,
    SourceRead,
    RemoteRead,
    Build,
    Test,
    RepositoryEdit,
}

impl Class {
    pub const ALL: [Self; 14] = [
        Self::Launch,
        Self::Send,
        Self::Ask,
        Self::Stop,
        Self::Adopt,
        Self::Gate,
        Self::IssueBox,
        Self::Status,
        Self::PlacementLook,
        Self::SourceRead,
        Self::RemoteRead,
        Self::Build,
        Self::Test,
        Self::RepositoryEdit,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Send => "send",
            Self::Ask => "ask",
            Self::Stop => "stop",
            Self::Adopt => "adopt",
            Self::Gate => "gate",
            Self::IssueBox => "issue-box",
            Self::Status => "status",
            Self::PlacementLook => "placement-look",
            Self::SourceRead => "source-read",
            Self::RemoteRead => "remote-read",
            Self::Build => "build",
            Self::Test => "test",
            Self::RepositoryEdit => "repository-edit",
        }
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.name() == name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Owner {
    Proxy,
    Worker,
}

impl Owner {
    pub fn name(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Worker => "worker",
        }
    }

    fn named(name: &str) -> Option<Self> {
        [Self::Proxy, Self::Worker].into_iter().find(|owner| owner.name() == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub class: Class,
    pub owner: Owner,
    pub what: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Table(Vec<Row>);

impl Table {
    pub fn load() -> Result<Self, String> {
        Self::read(document(DEFAULTS))
    }

    pub fn read(defaults: &DocumentMut) -> Result<Self, String> {
        let rows = defaults.get(TABLE).and_then(|entry| entry.get("value")).and_then(|value| value.as_array()).ok_or_else(|| format!("{DEFAULTS} has no {TABLE} list"))?;
        let mut table = Vec::new();
        for row in rows {
            let row = Self::row(row)?;
            if table.iter().any(|seen: &Row| seen.class == row.class) {
                return Err(format!("{TABLE} names {} twice", row.class.name()));
            }
            table.push(row);
        }
        if let Some(missing) = Class::ALL.into_iter().find(|class| !table.iter().any(|row| row.class == *class)) {
            return Err(format!("{TABLE} has no row for {}", missing.name()));
        }
        table.sort_by_key(|row| row.class);
        Ok(Self(table))
    }

    fn row(row: &Value) -> Result<Row, String> {
        let row = row.as_inline_table().ok_or_else(|| format!("{TABLE} holds a row that is not a table"))?;
        let text = |key: &str| row.get(key).and_then(Value::as_str).ok_or_else(|| format!("{TABLE} holds a row without {key}"));
        let class = text("class")?;
        let class = Class::named(class).ok_or_else(|| format!("{TABLE} names {class}, which the proxy-role guard does not sort calls into"))?;
        let owner = text("owner")?;
        let owner = Owner::named(owner).ok_or_else(|| format!("{TABLE} gives {} to {owner}, which is neither proxy nor worker", class.name()))?;
        Ok(Row { class, owner, what: text("what")?.to_owned() })
    }

    pub fn owner(&self, class: Class) -> Owner {
        self.0.iter().find(|row| row.class == class).map_or(Owner::Worker, |row| row.owner)
    }

    pub fn rows(&self) -> &[Row] {
        &self.0
    }

    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        for (owner, heading) in [(Owner::Proxy, "The proxy does itself:"), (Owner::Worker, "The proxy writes a brief and launches a worker for:")] {
            lines.push(heading.to_owned());
            lines.extend(self.0.iter().filter(|row| row.owner == owner).map(|row| format!("  {:<16} {}", row.class.name(), row.what)));
        }
        lines.join("\n")
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north config proxy role");
        return 2;
    }
    match Table::load() {
        Ok(table) => {
            println!("{}", table.render());
            0
        }
        Err(fault) => {
            eprintln!("north config proxy role: {fault}");
            1
        }
    }
}
