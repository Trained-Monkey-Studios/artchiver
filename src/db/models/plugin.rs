use artchiver_sdk::ConfigValue;
use rusqlite::{
    ToSql,
    types::{ToSqlOutput, Value},
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct PluginId(i64);
impl PluginId {
    pub fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        row.get("id").map(Self)
    }
}
impl ToSql for PluginId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(self.0)))
    }
}
impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PluginId({})", self.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbPlugin {
    id: PluginId,
    name: String,
    configs: Vec<(String, ConfigValue)>,
}

impl DbPlugin {
    pub fn new(id: i64, name: String, configs: Vec<(String, ConfigValue)>) -> Self {
        Self {
            id: PluginId(id),
            name,
            configs,
        }
    }

    pub fn id(&self) -> PluginId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn configs(&self) -> &[(String, ConfigValue)] {
        &self.configs
    }
}
