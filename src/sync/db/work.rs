use crate::shared::progress::HostUpdateSender;
use crate::shared::update::DataUpdate;
use crate::{
    shared::progress::{LogSender, ProgressSender},
    sync::db::model::string_to_rarray,
};
use anyhow::{Result, ensure};
use artchiver_sdk::Work;
use jiff::civil::Date;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct WorkId(i64);

// DB-centered [art]work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbWork {
    id: WorkId,
    name: String,
    artist_id: i64,
    date: Date,
    preview_url: String,
    screen_url: String,
    archive_url: Option<String>,

    preview_path: Option<PathBuf>,
    screen_path: Option<PathBuf>,
    archive_path: Option<PathBuf>,

    tags: Vec<String>,
}

impl DbWork {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: WorkId(row.get("id")?),
            name: row.get("name")?,
            artist_id: row.get("artist_id")?,
            date: row.get("date")?,
            preview_url: row.get("preview_url")?,
            screen_url: row.get("screen_url")?,
            archive_url: row.get("archive_url")?,
            preview_path: row
                .get::<&str, Option<String>>("preview_path")?
                .map(|s| s.into()),
            screen_path: row
                .get::<&str, Option<String>>("screen_path")?
                .map(|s| s.into()),
            archive_path: row
                .get::<&str, Option<String>>("archive_path")?
                .map(|s| s.into()),
            tags: Vec::new(),
        })
    }

    pub fn id(&self) -> WorkId {
        self.id
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn date(&self) -> &Date {
        &self.date
    }

    pub fn preview_url(&self) -> &str {
        self.preview_url.as_str()
    }

    pub fn screen_url(&self) -> &str {
        self.screen_url.as_str()
    }

    pub fn archive_url(&self) -> Option<&str> {
        self.archive_url.as_deref()
    }

    pub fn preview_path(&self) -> Option<&Path> {
        self.preview_path.as_deref()
    }

    pub fn screen_path(&self) -> Option<&Path> {
        self.screen_path.as_deref()
    }

    pub fn archive_path(&self) -> Option<&Path> {
        self.archive_path.as_deref()
    }

    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(|s| s.as_str())
    }

    // For updating inline in the UX when the UX gets a download ready notice.
    pub fn set_paths(
        &mut self,
        preview_path: PathBuf,
        screen_path: PathBuf,
        archive_path: Option<PathBuf>,
    ) {
        self.preview_path = Some(preview_path);
        self.screen_path = Some(screen_path);
        self.archive_path = archive_path;
    }
}

pub fn upsert_works(
    mut conn: PooledConnection<SqliteConnectionManager>,
    works: &[Work],
    log: &mut LogSender,
    progress: &mut ProgressSender,
    host: &mut HostUpdateSender,
) -> Result<()> {
    let total_count = works.len();
    let mut current_pos = 0;
    log.info(format!("Writing {total_count} works to the database..."));

    for chunk in works.chunks(1_000) {
        log.trace(format!("db->upsert_works chunk of {}", chunk.len()));
        let mut xaction = conn.transaction()?;
        {
            let mut insert_work_stmt = xaction.prepare("INSERT OR IGNORE INTO works (name, artist_id, date, preview_url, screen_url, archive_url) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")?;
            let mut select_tag_ids_from_names =
                xaction.prepare("SELECT id FROM tags WHERE name IN rarray(?)")?;
            let mut insert_work_tag_stmt = xaction
                .prepare("INSERT OR IGNORE INTO work_tags (tag_id, work_id) VALUES (?, ?)")?;
            let mut select_work_id_stmt = xaction.prepare("SELECT id FROM works WHERE name = ?")?;

            for work in chunk.iter() {
                let work_id = if let Ok(work_id) = insert_work_stmt.query_one(
                    params![
                        work.name(),
                        0, // TODO: artist_id
                        work.date(),
                        work.preview_url(),
                        work.screen_url(),
                        work.archive_url()
                    ],
                    |row| row.get::<usize, i64>(0),
                ) {
                    work_id
                } else if let Ok(work_id) =
                    select_work_id_stmt.query_row(params![work.name()], |row| row.get(0))
                {
                    work_id
                } else {
                    log.warn(format!(
                        "Detected duplicate URL in work {}, skipping",
                        work.name()
                    ));
                    continue;
                };
                let tag_ids: Vec<i64> = select_tag_ids_from_names
                    .query_map([string_to_rarray(work.tags())], |row| row.get(0))?
                    .flatten()
                    .collect();
                for tag_id in &tag_ids {
                    insert_work_tag_stmt.execute(params![*tag_id, work_id])?;
                }
            }
        }
        xaction.commit()?;

        current_pos += chunk.len();
        progress.set_percent(current_pos, total_count);
    }

    Ok(())
}

pub fn update_work_paths(
    conn: PooledConnection<SqliteConnectionManager>,
    screen_url: &str,
    preview_path: &str,
    screen_path: &str,
    archive_path: Option<&str>,
    host: &mut HostUpdateSender,
) -> Result<()> {
    assert!(!screen_url.is_empty(), "have a path for empty screen url");
    assert!(!preview_path.is_empty(), "empty preview path");
    assert!(!screen_path.is_empty(), "empty screen path");
    let work_id: i64 = conn.query_one(
        "SELECT id FROM works WHERE screen_url = ?",
        [screen_url],
        |row| row.get(0),
    )?;
    let row_cnt = conn.execute(
        "UPDATE works SET preview_path = ?, screen_path = ?, archive_path = ? WHERE id = ?",
        params![preview_path, screen_path, archive_path, work_id],
    )?;
    ensure!(row_cnt == 1);
    host.note_completed_download(WorkId(work_id), preview_path, screen_path, archive_path)?;
    Ok(())
}

pub fn list_works_with_any_tags(
    conn: PooledConnection<SqliteConnectionManager>,
    tags: &[String],
) -> Result<Vec<DbWork>> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }

    // If we decide we *have* to apply AND up front, it looks like this.
    // GROUP BY works.id HAVING COUNT(DISTINCT tags.name) = {enabled_size}
    let query = r#"SELECT works.* FROM works
            INNER JOIN work_tags ON work_tags.work_id = works.id
            INNER JOIN tags ON work_tags.tag_id = tags.id
            WHERE tags.name IN rarray(?)"#;
    // dbg!(query.replace("\n            ", " "));
    let mut stmt = conn.prepare(query)?;
    Ok(stmt
        .query_map([string_to_rarray(tags)], DbWork::from_row)?
        .flatten()
        .collect())
}
