use crate::{
    db::{
        model::string_to_rarray,
        models::{plugin::PluginId, work::WorkId},
    },
    shared::{
        progress::{HostUpdateSender, LogSender, ProgressSender, UpdateSource},
        update::DataUpdate,
    },
};
use anyhow::{Result, ensure};
use artchiver_sdk::{Tag, Work};
use crossbeam::channel::{Receiver, Sender};
use log::error;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

pub enum DbWriterRequest {
    UpsertTags {
        plugin_id: PluginId,
        tags: Vec<Tag>,
    },
    UpsertWorks {
        plugin_id: PluginId,
        for_tag: String,
        works: Vec<Work>,
    },
    SetWorkDownloadPaths {
        screen_url: String,
        preview_path: String,
        screen_path: String,
        archive_path: Option<String>,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct DbWriteHandle {
    tx_to_writer: Sender<DbWriterRequest>,
}

impl DbWriteHandle {
    pub fn new(tx_to_writer: Sender<DbWriterRequest>) -> Self {
        Self { tx_to_writer }
    }

    pub fn send_exit_request(&self) {
        self.tx_to_writer
            .send(DbWriterRequest::Shutdown)
            .expect("writer send died at exit");
    }

    pub fn upsert_tags(&self, plugin_id: PluginId, tags: Vec<Tag>) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::UpsertTags { plugin_id, tags })?;
        Ok(())
    }

    pub fn upsert_works(&self, plugin_id: PluginId, tag: &str, works: Vec<Work>) -> Result<()> {
        self.tx_to_writer.send(DbWriterRequest::UpsertWorks {
            plugin_id,
            for_tag: tag.to_owned(),
            works,
        })?;
        Ok(())
    }

    pub fn set_work_download_paths(
        &self,
        screen_url: &str,
        preview_path: String,
        screen_path: String,
        archive_path: Option<String>,
    ) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkDownloadPaths {
                screen_url: screen_url.to_owned(),
                preview_path,
                screen_path,
                archive_path,
            })?;
        Ok(())
    }
}

pub struct DbBgWriter {
    pool: r2d2::Pool<SqliteConnectionManager>,
    rx_from_app: Receiver<DbWriterRequest>,
    tx_to_app: Sender<DataUpdate>,
}

impl DbBgWriter {
    pub fn new(
        pool: r2d2::Pool<SqliteConnectionManager>,
        rx_from_app: Receiver<DbWriterRequest>,
        tx_to_app: Sender<DataUpdate>,
    ) -> Self {
        Self {
            pool,
            rx_from_app,
            tx_to_app,
        }
    }

    pub fn main(&mut self) -> Result<()> {
        loop {
            match self.rx_from_app.recv() {
                Ok(DbWriterRequest::Shutdown) => {
                    break;
                }
                Ok(msg) => self.handle_message(msg)?,
                Err(e) => {
                    error!("Database bg thread recv error: {e}");
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn handle_message(&mut self, msg: DbWriterRequest) -> Result<()> {
        let mut log = LogSender::wrap(UpdateSource::DbWriter, self.tx_to_app.clone());
        let mut progress = ProgressSender::wrap(UpdateSource::DbWriter, self.tx_to_app.clone());
        let mut host = HostUpdateSender::wrap(UpdateSource::DbWriter, self.tx_to_app.clone());
        match msg {
            DbWriterRequest::Shutdown => panic!("expected exit to be handled in main"),
            DbWriterRequest::UpsertTags { plugin_id, tags } => {
                upsert_tags(
                    &mut self.pool.get()?,
                    plugin_id,
                    &tags,
                    &mut log,
                    &mut progress,
                )?;
                host.note_tags_were_refreshed()?;
            }
            DbWriterRequest::UpsertWorks {
                plugin_id: _,
                for_tag,
                works,
            } => {
                upsert_works(self.pool.get()?, &works, &mut log, &mut progress)?;
                host.note_works_were_refreshed(for_tag)?;
            }
            DbWriterRequest::SetWorkDownloadPaths {
                screen_url,
                preview_path,
                screen_path,
                archive_path,
            } => {
                update_work_paths(
                    &self.pool.get()?,
                    &screen_url,
                    &preview_path,
                    &screen_path,
                    archive_path.as_deref(),
                    &mut host,
                )?;
            }
        }
        Ok(())
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

pub fn upsert_works(
    mut conn: PooledConnection<SqliteConnectionManager>,
    works: &[Work],
    log: &mut LogSender,
    progress: &mut ProgressSender,
) -> Result<()> {
    let total_count = works.len();
    let mut current_pos = 0;
    log.info(format!("Writing {total_count} works to the database..."));

    for chunk in works.chunks(1_000) {
        log.trace(format!("db->upsert_works chunk of {}", chunk.len()));
        let xaction = conn.transaction()?;
        {
            let mut insert_work_stmt = xaction.prepare("INSERT OR IGNORE INTO works (name, artist_id, date, preview_url, screen_url, archive_url) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")?;
            let mut select_tag_ids_from_names =
                xaction.prepare("SELECT id FROM tags WHERE name IN rarray(?)")?;
            let mut insert_work_tag_stmt = xaction
                .prepare("INSERT OR IGNORE INTO work_tags (tag_id, work_id) VALUES (?, ?)")?;
            let mut select_work_id_stmt = xaction.prepare("SELECT id FROM works WHERE name = ?")?;

            for work in chunk {
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
                log::trace!("db->upsert_works with tags: {:?}", work.tags());
                log.trace(format!("db->upsert_works with tags: {:?}", work.tags()));
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
    conn: &PooledConnection<SqliteConnectionManager>,
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
    host.note_completed_download(
        WorkId::wrap(work_id),
        preview_path,
        screen_path,
        archive_path,
    )?;
    Ok(())
}
