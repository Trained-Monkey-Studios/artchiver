use artchiver_sdk::TagKind;
use rusqlite::{
    Row, ToSql,
    types::{ToSqlOutput, Value},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct TagId(i64);
impl ToSql for TagId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(self.0)))
    }
}
impl TagId {
    pub fn wrap(id: i64) -> Self {
        Self(id)
    }
}

// A DB sourced tag
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbTag {
    id: TagId,
    name: String,
    kind: TagKind,
    network_count: u64,
    local_count: Option<u64>,
    hidden: bool,
    favorite: bool,
    sources: Vec<String>,
}

impl DbTag {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: TagId(row.get("id")?),
            name: row.get("name")?,
            kind: row
                .get::<&str, String>("kind")?
                .parse()
                .ok()
                .unwrap_or_default(),
            network_count: row.get("network_count")?,
            local_count: None,
            hidden: row.get("hidden").ok().unwrap_or(false),
            favorite: row.get("favorite")?,
            sources: row
                .get::<&str, String>("plugin_names")?
                .split(',')
                .map(|s| s.to_owned())
                .collect(),
        })
    }

    pub fn set_local_count(&mut self, actual_work_count: u64) {
        self.local_count = Some(actual_work_count);
    }

    pub fn id(&self) -> TagId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> TagKind {
        self.kind
    }

    pub fn network_count(&self) -> u64 {
        self.network_count
    }

    pub fn local_count(&self) -> Option<u64> {
        self.local_count
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn favorite(&self) -> bool {
        self.favorite
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }
}
