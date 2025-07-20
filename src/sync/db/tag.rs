use crate::{shared::progress::ProgressSender, sync::db::model::OrderDir};
use artchiver_sdk::{TagInfo, TagKind};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt, ops::Range};

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TagSortCol {
    #[default]
    Name,
    Count,
}

impl fmt::Display for TagSortCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Name => "tags.name",
            Self::Count => "SUM(plugin_tags.presumed_work_count)",
        };
        write!(f, "{s}")
    }
}

impl TagSortCol {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut selected = match self {
            Self::Name => 0,
            Self::Count => 1,
        };
        let options = ["Name", "Count"];
        egui::ComboBox::new("tag_order_column", "Column")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, options.len(), |i| options[i]);
        *self = match selected {
            0 => Self::Name,
            1 => Self::Count,
            _ => panic!("invalid column selected"),
        };
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TagOrder {
    column: TagSortCol,
    order: OrderDir,
}

impl fmt::Display for TagOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ORDER BY {} {}", self.column, self.order)
    }
}

impl TagOrder {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.column.ui(ui);
        self.order.ui("tags", ui);
    }
}

pub fn upsert_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    plugin_id: i64,
    tags: &[TagInfo],
    progress: &mut ProgressSender,
) -> anyhow::Result<()> {
    progress.set_spinner();

    let mut insert_tag_stmt =
        conn.prepare("INSERT OR IGNORE INTO tags (name, kind, wiki_url) VALUES (?, ?, ?)")?;
    let mut select_tag_id_stmt = conn.prepare("SELECT id FROM tags WHERE name = ?")?;
    let mut insert_plugin_tag_stmt =
        conn.prepare("INSERT OR IGNORE INTO plugin_tags (plugin_id, tag_id, presumed_work_count) VALUES (?, ?, ?)")?;

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
                tag.wiki_url(),
            ])?;
            let tag_id = if row_cnt > 0 {
                conn.last_insert_rowid()
            } else {
                select_tag_id_stmt.query_row(params![tag.name()], |row| row.get(0))?
            };
            insert_plugin_tag_stmt.execute(params![
                plugin_id,
                tag_id,
                tag.presumed_work_count(),
            ])?;
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
    source: Option<&str>,
) -> anyhow::Result<i64> {
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

pub fn list_tags(
    conn: &PooledConnection<SqliteConnectionManager>,
    range: Range<usize>,
    filter: &str,
    source: Option<&str>,
    order: TagOrder,
) -> anyhow::Result<Vec<TagEntry>> {
    // SELECT id,name,kind,actual_work_count,SUM(presumed_work_count) FROM
    //   (SELECT tags.id, tags.name, tags.kind, plugin_tags.presumed_work_count, tags.hidden, tags.favorite, COUNT(work_tags.id) as actual_work_count
    //   FROM tags LEFT JOIN work_tags ON tags.id == work_tags.tag_id LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
    //   WHERE tags.name LIKE 'Lovers' AND plugins.name LIKE '%' GROUP BY tags.name, plugin_tags.presumed_work_count ORDER BY tags.name ASC LIMIT 10 OFFSET 0) GROUP BY name;
    // id|name|kind|actual_work_count|SUM(presumed_work_count)
    // 1132|Lovers|default|117|666
    let mut stmt = conn.prepare(&format!(
        r#"SELECT id, name, kind, favorite, actual_work_count, SUM(presumed_work_count) FROM
            (SELECT tags.id, tags.name, tags.kind, plugin_tags.presumed_work_count, tags.hidden, tags.favorite, COUNT(work_tags.id) as actual_work_count
                FROM tags
                LEFT JOIN work_tags ON tags.id == work_tags.tag_id
                LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id
                LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
                WHERE tags.name LIKE ? AND plugins.name LIKE ? AND plugin_tags.presumed_work_count > 0
                GROUP BY tags.name, plugin_tags.presumed_work_count
                {order}
                LIMIT ? OFFSET ?)
            GROUP BY name;
        "#,
    ))?;
    let rows = stmt.query_map(
        params![
            format!("%{filter}%"),
            source.unwrap_or("%"),
            range.end - range.start,
            range.start
        ],
        |row| {
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
            Ok(TagEntry::new(
                id,
                name,
                kind,
                presumed_work_count,
                actual_work_count,
                false,
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
