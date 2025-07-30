use crate::{
    shared::progress::{LogSender, ProgressSender},
    sync::db::plugin::PluginId,
};
use anyhow::Result;
use artchiver_sdk::{Tag, TagKind};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{
    Row, ToSql, params,
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

pub fn upsert_tags(
    conn: &mut PooledConnection<SqliteConnectionManager>,
    plugin_id: PluginId,
    tags: &[Tag],
    log: &mut LogSender,
    progress: &mut ProgressSender,
) -> Result<()> {
    progress.set_spinner();

    let total_count = tags.len();
    let mut current_pos = 0;
    log.info(format!("Writing {total_count} tags to the database..."));
    for chunk in tags.chunks(10_000) {
        log.trace(format!("db->upsert_tags chunk of {}", chunk.len()));
        let xaction = conn.transaction()?;
        {
            let mut insert_tag_stmt = xaction
                .prepare("INSERT OR IGNORE INTO tags (name, kind, wiki_url) VALUES (?, ?, ?)")?;
            let mut select_tag_id_stmt = xaction.prepare("SELECT id FROM tags WHERE name = ?")?;
            let mut insert_plugin_tag_stmt =
            xaction.prepare("INSERT OR IGNORE INTO plugin_tags (plugin_id, tag_id, presumed_work_count) VALUES (?, ?, ?)")?;

            // conn.execute("BEGIN TRANSACTION", ())?;
            for tag in chunk {
                let row_cnt = insert_tag_stmt.execute(params![
                    tag.name(),
                    tag.kind().to_string(),
                    tag.wiki_url(),
                ])?;
                let tag_id = if row_cnt > 0 {
                    xaction.last_insert_rowid()
                } else {
                    select_tag_id_stmt.query_row(params![tag.name()], |row| row.get(0))?
                };
                insert_plugin_tag_stmt.execute(params![
                    plugin_id,
                    tag_id,
                    tag.presumed_work_count(),
                ])?;
            }
        }
        xaction.commit()?;
        // conn.execute("COMMIT TRANSACTION", ())?;

        current_pos += chunk.len();
        progress.set_percent(current_pos, total_count);
    }

    progress.clear();
    Ok(())
}

pub fn list_all_tags(conn: &PooledConnection<SqliteConnectionManager>) -> Result<Vec<DbTag>> {
    let query = r#"
    SELECT tags.id, tags.name, tags.kind, tags.wiki_url, tags.favorite,
            SUM(plugin_tags.presumed_work_count) AS network_count,
            GROUP_CONCAT(plugins.name) AS plugin_names
        FROM tags
        LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id
        LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
        WHERE tags.hidden = 0
        GROUP BY tags.name, plugin_tags.presumed_work_count;"#;
    let mut stmt = conn.prepare(query)?;
    let tags: Vec<DbTag> = stmt.query_map((), DbTag::from_row)?.flatten().collect();
    Ok(tags)
}

pub fn count_works_per_tag(
    conn: &PooledConnection<SqliteConnectionManager>,
) -> Result<Vec<(TagId, u64)>> {
    let query = r#"SELECT tags.id, COUNT(work_tags.id)
        FROM tags
        LEFT JOIN work_tags ON tags.id == work_tags.tag_id
        WHERE tags.hidden = 0
        GROUP BY tags.id;"#;
    let out = conn
        .prepare(query)?
        .query_map((), |row| {
            let tag_id = TagId(row.get(0)?);
            let count = row.get(1)?;
            Ok((tag_id, count))
        })?
        .flatten()
        .collect();
    Ok(out)
}
