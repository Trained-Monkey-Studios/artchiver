use crate::{
    shared::{
        progress::{HostUpdateSender, LogSender, ProgressSender, UpdateSource},
        update::DataUpdate,
    },
    sync::db::{
        handle::DbWriterRequest,
        tag::upsert_tags,
        work::{update_work_paths, upsert_works},
    },
};
use anyhow::Result;
use crossbeam::channel::{Receiver, Sender};
use log::error;
use r2d2_sqlite::SqliteConnectionManager;

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
                    self.pool.get()?,
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
