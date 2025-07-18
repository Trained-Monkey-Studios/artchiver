use crate::shared::progress::ProgressSender;
use artchiver_sdk::{TagInfo, TagKind};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Range;

// A DB sourced tag
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TagEntry {
    id: i64,
    name: String,
    kind: TagKind,
    presumed_work_count: Option<u64>,
    actual_work_count: u64,
    hidden: bool,
    favorite: bool,
}

impl TagEntry {
    pub fn new(
        id: i64,
        name: String,
        kind: TagKind,
        presumed_work_count: Option<u64>,
        actual_work_count: u64,
        hidden: bool,
        favorite: bool,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            presumed_work_count,
            actual_work_count,
            hidden,
            favorite,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> TagKind {
        self.kind
    }

    pub fn presumed_work_count(&self) -> Option<u64> {
        self.presumed_work_count
    }

    pub fn actual_work_count(&self) -> u64 {
        self.actual_work_count
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn favorite(&self) -> bool {
        self.favorite
    }
}

pub fn upsert_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    plugin_id: i64,
    tags: &[TagInfo],
    progress: &mut ProgressSender,
) -> anyhow::Result<()> {
    progress.set_spinner();

    let mut insert_tag_stmt = conn
        .prepare("INSERT OR IGNORE INTO tags (name, kind, presumed_work_count) VALUES (?, ?, ?)")?;
    let mut select_tag_id_stmt = conn.prepare("SELECT id FROM tags WHERE name = ?")?;
    let mut insert_plugin_tag_stmt =
        conn.prepare("INSERT OR IGNORE INTO plugin_tags (plugin_id, tag_id) VALUES (?, ?)")?;

    let total_count = tags.len();
    let mut current_pos = 0;
    progress.info(format!("Writing {total_count} tags to the database..."));
    for chunk in tags.chunks(10_000) {
        progress.trace(format!("db->upsert_tags chunk of {}", chunk.len()));
        conn.execute("BEGIN TRANSACTION", ())?;
        for tag in chunk {
            let row_cnt = insert_tag_stmt.execute(params![
                tag.name(),
                tag.kind().to_string(),
                tag.presumed_work_count(),
            ])?;
            let tag_id = if row_cnt > 0 {
                conn.last_insert_rowid()
            } else {
                select_tag_id_stmt.query_row(params![tag.name()], |row| row.get(0))?
            };
            insert_plugin_tag_stmt.execute(params![plugin_id, tag_id])?;
        }
        conn.execute("COMMIT TRANSACTION", ())?;

        current_pos += chunk.len();
        progress.set_percent(current_pos, total_count);
        progress.database_changed()?;
    }

    progress.clear();
    Ok(())
}

pub fn count_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    filter: &str,
) -> anyhow::Result<i64> {
    let cnt = conn.query_row(
        "SELECT COUNT(*) FROM tags WHERE name LIKE ? ORDER BY name ASC",
        [format!("%{filter}%")],
        |row| row.get(0),
    )?;
    Ok(cnt)
}

pub fn list_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    range: Range<usize>,
    filter: &str,
) -> anyhow::Result<Vec<TagEntry>> {
    let mut stmt = conn.prepare(
        r#"SELECT tags.id, tags.name, tags.kind, tags.presumed_work_count, tags.hidden, tags.favorite, COUNT(work_tags.id)
            FROM tags
            LEFT JOIN work_tags ON tags.id == work_tags.tag_id
            WHERE tags.name LIKE ?
            GROUP BY tags.name
            ORDER BY tags.name ASC
            LIMIT ? OFFSET ?"#,
    )?;
    let rows = stmt.query_map(
        params![format!("%{filter}%"), range.end - range.start, range.start],
        |row| {
            let id = row.get(0)?;
            let name = row.get(1)?;
            let kind = row
                .get::<usize, String>(2)?
                .parse()
                .ok()
                .unwrap_or_default();
            let presumed_work_count = row.get(3)?;
            let hidden = row.get(4)?;
            let favorite = row.get(5)?;
            let actual_work_count = row.get(6)?;
            Ok(TagEntry::new(
                id,
                name,
                kind,
                presumed_work_count,
                actual_work_count,
                hidden,
                favorite,
            ))
        },
    )?;
    Ok(rows.flatten().collect())
}

pub fn list_plugins_for_tag(
    conn: &PooledConnection<SqliteConnectionManager>,
    tag: &str,
) -> anyhow::Result<HashSet<String>> {
    let tag_id: i64 = conn.query_one("SELECT id FROM tags WHERE name = ?", [tag], |row| {
        row.get(0)
    })?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.name FROM plugins AS p INNER JOIN plugin_tags AS pt WHERE pt.tag_id = ? AND p.id = pt.plugin_id",
    )?;
    let rows = stmt.query_map(params![tag_id], |row| row.get(0))?;
    Ok(rows.flatten().collect())
}
