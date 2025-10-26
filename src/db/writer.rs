use crate::{
    db::{
        model::{DbCancellation, string_to_rarray},
        models::{plugin::PluginId, tag::TagId, work::WorkId},
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
    SetWorkFavorite {
        work_id: WorkId,
        favorite: bool,
    },
    SetWorkHidden {
        work_id: WorkId,
        hidden: bool,
    },
    SetTagFavorite {
        tag_id: TagId,
        favorite: bool,
    },
    SetTagHidden {
        tag_id: TagId,
        hidden: bool,
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

    pub fn set_work_favorite(&self, work_id: WorkId, favorite: bool) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkFavorite { work_id, favorite })?;
        Ok(())
    }

    pub fn set_work_hidden(&self, work_id: WorkId, hidden: bool) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkHidden { work_id, hidden })?;
        Ok(())
    }

    pub fn set_tag_favorite(&self, tag_id: TagId, favorite: bool) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetTagFavorite { tag_id, favorite })?;
        Ok(())
    }

    pub fn set_tag_hidden(&self, tag_id: TagId, hidden: bool) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetTagHidden { tag_id, hidden })?;
        Ok(())
    }
}

pub struct DbBgWriter {
    pool: r2d2::Pool<SqliteConnectionManager>,
    db_cancellation: DbCancellation,
    rx_from_app: Receiver<DbWriterRequest>,
    tx_to_app: Sender<DataUpdate>,
}

impl DbBgWriter {
    pub fn new(
        pool: r2d2::Pool<SqliteConnectionManager>,
        db_cancellation: DbCancellation,
        rx_from_app: Receiver<DbWriterRequest>,
        tx_to_app: Sender<DataUpdate>,
    ) -> Self {
        Self {
            pool,
            db_cancellation,
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
                upsert_works(
                    self.pool.get()?,
                    &self.db_cancellation,
                    &works,
                    &mut log,
                    &mut progress,
                )?;
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
            DbWriterRequest::SetWorkFavorite { work_id, favorite } => {
                set_work_favorite(&self.pool.get()?, work_id, favorite)?;
                host.note_work_favorite_status_changed(work_id, favorite)?;
            }
            DbWriterRequest::SetWorkHidden { work_id, hidden } => {
                set_work_hidden(&self.pool.get()?, work_id, hidden)?;
                host.note_work_hidden_status_changed(work_id, hidden)?;
            }
            DbWriterRequest::SetTagFavorite { tag_id, favorite } => {
                log.info(format!("Setting tag {tag_id} to favorite: {favorite}"));
                set_tag_favorite(&self.pool.get()?, tag_id, favorite)?;
                host.note_tag_favorite_status_changed(tag_id, favorite)?;
            }
            DbWriterRequest::SetTagHidden { tag_id, hidden } => {
                log.info(format!("Setting tag {tag_id} to hidden: {hidden}"));
                set_tag_hidden(&self.pool.get()?, tag_id, hidden)?;
                host.note_tag_hidden_status_changed(tag_id, hidden)?;
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
    log.info(format!(
        "Writing {total_count} tags for plugin {plugin_id} to the database..."
    ));
    for chunk in tags.chunks(10_000) {
        let mut tag_ids = Vec::new();

        log.trace(format!("db->upsert_tags chunk of {}", chunk.len()));
        let xaction = conn.transaction()?;
        {
            let mut insert_tag_stmt = xaction
                .prepare("INSERT INTO tags (name, kind, wiki_url) VALUES (?, ?, ?) ON CONFLICT DO UPDATE SET kind = ?, wiki_url = ? WHERE tags.name = ?")?;
            let mut select_tag_id_stmt = xaction.prepare("SELECT id FROM tags WHERE name = ?")?;

            for tag in chunk {
                let row_cnt = insert_tag_stmt.execute(params![
                    tag.name(),
                    tag.kind().to_string(),
                    tag.wiki_url(),
                    tag.kind().to_string(),
                    tag.wiki_url(),
                    tag.name(),
                ])?;
                ensure!(row_cnt == 1, "failed to insert tag");
                let mut tag_id = xaction.last_insert_rowid();
                if tag_id == 0 {
                    tag_id = select_tag_id_stmt.query_row(params![tag.name()], |row| row.get(0))?;
                }
                tag_ids.push((tag_id, tag.presumed_work_count()));
            }
        }
        xaction.commit()?;

        let xaction = conn.transaction()?;
        {
            let mut insert_plugin_tag_stmt =
                xaction.prepare("INSERT INTO plugin_tags (plugin_id, tag_id, presumed_work_count) VALUES (?, ?, ?) ON CONFLICT DO UPDATE SET presumed_work_count = ?")?;

            for (tag_id, work_count) in &tag_ids {
                let row_cnt = insert_plugin_tag_stmt.execute(params![
                    plugin_id,
                    tag_id,
                    *work_count,
                    *work_count,
                ])?;
                ensure!(row_cnt == 1, "failed to insert plugin_tag binding");
            }
        }
        xaction.commit()?;

        current_pos += chunk.len();
        progress.set_percent(current_pos, total_count);
    }

    progress.clear();
    Ok(())
}

pub fn upsert_works(
    mut conn: PooledConnection<SqliteConnectionManager>,
    db_cancellation: &DbCancellation,
    works: &[Work],
    log: &mut LogSender,
    progress: &mut ProgressSender,
) -> Result<()> {
    let total_count = works.len();
    let mut current_pos = 0;
    log.info(format!("Writing {total_count} works to the database..."));

    for chunk in works.chunks(1_000) {
        if db_cancellation.is_cancelled() {
            log.warn("Exiting upsert_works early due to cancellation");
        }

        log.trace(format!("db->upsert_works chunk of {}", chunk.len()));
        let xaction = conn.transaction()?;
        {
            let mut insert_work_stmt = xaction.prepare(
                r#"
                INSERT OR REPLACE INTO works
                (
                    name, artist_id, date, preview_url, screen_url, archive_url,
                    location_custody, location_site, location_room, location_position, location_description, location_on_display,
                    history_attribution, history_attribution_sort_key, history_display_date, history_begin_year, history_end_year, history_provenance, history_credit_line,
                    physical_medium, physical_dimensions_display, physical_inscription, physical_markings, physical_watermarks
                )
                VALUES
                (?, ?, ?, ?, ?, ?,
                 ?, ?, ?, ?, ?, ?,
                 ?, ?, ?, ?, ?, ?, ?,
                 ?, ?, ?, ?, ?)
                RETURNING id"#,
            )?;
            let mut insert_measurement_stmt = xaction.prepare(r#"
                INSERT OR REPLACE INTO work_measurements (work_id, name, description, value, si_unit)
                VALUES (?, ?, ?, ?, ?)
            "#)?;
            let mut select_tag_ids_from_names =
                xaction.prepare("SELECT id FROM tags WHERE name IN rarray(?)")?;
            let mut insert_work_tag_stmt = xaction
                .prepare("INSERT OR IGNORE INTO work_tags (tag_id, work_id) VALUES (?, ?)")?;
            let mut select_work_id_stmt = xaction.prepare("SELECT id FROM works WHERE name = ?")?;

            for work in chunk {
                let params_array = params![
                    work.name(),
                    0, // TODO: artist_id
                    work.date(),
                    work.preview_url(),
                    work.screen_url(),
                    work.archive_url(),
                    work.location().map(|l| l.custody()),
                    work.location().map(|l| l.site()),
                    work.location().map(|l| l.room()),
                    work.location().map(|l| l.position()),
                    work.location().map(|l| l.description()),
                    work.location().map(|l| l.on_display()),
                    work.history().map(|h| h.attribution()),
                    work.history().map(|h| h.attribution_sort_key()),
                    work.history().map(|h| h.display_date()),
                    work.history().map(|h| h.begin_year()),
                    work.history().map(|h| h.end_year()),
                    work.history().map(|h| h.provenance()),
                    work.history().map(|h| h.credit_line()),
                    work.physical_data().map(|p| p.medium()),
                    work.physical_data().map(|p| p.dimensions_display()),
                    work.physical_data().map(|p| p.inscription()),
                    work.physical_data().map(|p| p.markings()),
                    work.physical_data().map(|p| p.watermarks()),
                ];
                let result =
                    insert_work_stmt.query_one(params_array, |row| row.get::<usize, i64>(0));
                let work_id = match result {
                    Ok(work_id) => work_id,
                    Err(err) => {
                        log.info(format!(
                            "Detected duplicate URL in work {}, {err:?}",
                            work.name()
                        ));
                        select_work_id_stmt.query_row(params![work.name()], |row| row.get(0))?
                    }
                };

                if let Some(physical) = work.physical_data() {
                    for measure in physical.measurements() {
                        insert_measurement_stmt.insert(params![
                            work_id,
                            measure.name(),
                            measure.description(),
                            measure.value(),
                            measure.si_unit().to_string()
                        ])?;
                    }
                }

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

fn set_work_favorite(
    conn: &PooledConnection<SqliteConnectionManager>,
    work_id: WorkId,
    favorite: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE works SET favorite = ? WHERE id = ?",
        params![favorite, work_id],
    )?;
    Ok(())
}

fn set_work_hidden(
    conn: &PooledConnection<SqliteConnectionManager>,
    work_id: WorkId,
    hidden: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE works SET hidden = ? WHERE id = ?",
        params![hidden, work_id],
    )?;
    Ok(())
}

fn set_tag_favorite(
    conn: &PooledConnection<SqliteConnectionManager>,
    tag_id: TagId,
    favorite: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE tags SET favorite = ? WHERE id = ?",
        params![favorite, tag_id],
    )?;
    Ok(())
}

fn set_tag_hidden(
    conn: &PooledConnection<SqliteConnectionManager>,
    tag_id: TagId,
    hidden: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE tags SET hidden = ? WHERE id = ?",
        params![hidden, tag_id],
    )?;
    Ok(())
}
