use crate::shared::progress::ProgressSender;
use crate::shared::update::DataUpdate;
use crate::sync::db::handle::DbWriterRequest;
use crate::sync::db::tag::upsert_tags;
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
                Ok(DbWriterRequest::Exit) => {
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
        match msg {
            DbWriterRequest::Exit => panic!("expected exit to be handled in main"),
            DbWriterRequest::UpsertTags { plugin_id, tags } => {
                upsert_tags(
                    &mut self.pool.get()?,
                    plugin_id,
                    &tags,
                    &mut ProgressSender::wrap(self.tx_to_app.clone()),
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
