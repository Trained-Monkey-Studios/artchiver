use crate::shared::progress::ProgressSender;
use anyhow::Result;
use artchiver_sdk::{Tag, TagKind};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

// A DB sourced tag
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbTag {
    id: i64,
    name: String,
    kind: TagKind,
    presumed_work_count: Option<u64>,
    actual_work_count: Option<u64>,
    hidden: bool,
    favorite: bool,
}

impl DbTag {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            kind: row
                .get::<&str, String>("kind")?
                .parse()
                .ok()
                .unwrap_or_default(),
            presumed_work_count: None,
            actual_work_count: None,
            hidden: row.get("hidden")?,
            favorite: row.get("favorite")?,
        })
    }

    pub fn new(
        id: i64,
        name: String,
        kind: TagKind,
        presumed_work_count: u64,
        actual_work_count: u64,
        hidden: bool,
        favorite: bool,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            presumed_work_count: Some(presumed_work_count),
            actual_work_count: Some(actual_work_count),
            hidden,
            favorite,
        }
    }

    pub fn set_local_count(&mut self, actual_work_count: u64) {
        self.actual_work_count = Some(actual_work_count);
    }

    pub fn set_network_count(&mut self, presumed_work_count: u64) {
        self.presumed_work_count = Some(presumed_work_count);
    }

    pub fn id(&self) -> i64 {
        self.id
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

    pub fn actual_work_count(&self) -> Option<u64> {
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
    conn: &mut PooledConnection<SqliteConnectionManager>,
    plugin_id: i64,
    tags: &[Tag],
    progress: &mut ProgressSender,
) -> Result<Vec<DbTag>> {
    progress.set_spinner();

    let total_count = tags.len();
    let mut current_pos = 0;
    progress.info(format!("Writing {total_count} tags to the database..."));
    for chunk in tags.chunks(10_000) {
        progress.trace(format!("db->upsert_tags chunk of {}", chunk.len()));
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
    list_all_tags(conn)
}

pub fn count_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    filter: &str,
    source: Option<&str>,
) -> Result<i64> {
    let cnt = conn.query_row(
        r#"SELECT COUNT(*) FROM
        (SELECT tags.id
            FROM tags
            LEFT JOIN work_tags ON tags.id == work_tags.tag_id
            LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id
            LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
            WHERE tags.name LIKE ? AND plugins.name LIKE ? AND plugin_tags.presumed_work_count > 0
            GROUP BY tags.name
            ORDER BY tags.name ASC)"#,
        params![format!("%{filter}%"), source.unwrap_or("%"),],
        |row| row.get(0),
    )?;
    Ok(cnt)
}

pub fn list_all_tags(conn: &PooledConnection<SqliteConnectionManager>) -> Result<Vec<DbTag>> {
    let query = r#"SELECT id, name, kind, favorite, actual_work_count, SUM(presumed_work_count) FROM
            (SELECT tags.id, tags.name, tags.kind, plugin_tags.presumed_work_count, tags.hidden, tags.favorite, COUNT(work_tags.id) as actual_work_count
                FROM tags
                LEFT JOIN work_tags ON tags.id == work_tags.tag_id
                LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id
                LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
                GROUP BY tags.name, plugin_tags.presumed_work_count)
            GROUP BY name;
        "#;
    println!("{}", query.replace("\n            ", " "));
    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map((), |row| {
        let id = row.get(0)?;
        let name = row.get(1)?;
        let kind = row
            .get::<usize, String>(2)?
            .parse()
            .ok()
            .unwrap_or_default();
        let favorite = row.get(3)?;
        let actual_work_count = row.get(4)?;
        let presumed_work_count = row.get(5)?;
        Ok(DbTag::new(
            id,
            name,
            kind,
            presumed_work_count,
            actual_work_count,
            false,
            favorite,
        ))
    })?;
    Ok(rows.flatten().collect())
}

/*
pub fn list_all_tags_1(conn: &PooledConnection<SqliteConnectionManager>) -> Result<Vec<DbTag>> {
    let start = Instant::now();
    let mut stmt = conn.prepare("SELECT * FROM tags")?;
    let mut tags = stmt.query_map((), |row| {
        DbTag::from_row(row)
    })?.flatten().map(|v| (v.id(), v)).collect::<HashMap<i64, DbTag>>();
    println!("Loaded {} tags in {:?}", tags.len(), start.elapsed());

    // Compute the actual number of works associated with each tag
    // This is the slow part of the query. Probably because there are millions of work_tags.
    let start = Instant::now();
    let query = r#"SELECT tags.id, COUNT(work_tags.id) as actual_work_count
                FROM tags
                LEFT JOIN work_tags ON tags.id == work_tags.tag_id
                GROUP BY tags.id;
        "#;
    println!("{}", query.replace("\n            ", " "));
    let mut stmt = conn.prepare(query)?;
    let actual_counts: Vec<(i64, u64)> = stmt.query_map((), |row| {
        let tag_id: i64 = row.get(0)?;
        let actual_count: u64 = row.get(1)?;
        Ok((tag_id, actual_count))
    })?.flatten().collect();
    println!("Updated {} count in {:?}", actual_counts.len(), start.elapsed());
    // for (tag_id, actual_count) in &actual_counts {
    //     tags.entry(*tag_id).and_modify(|t| t.actual_work_count = *actual_count);
    // }

    Ok(Vec::new().into())
}
 */
