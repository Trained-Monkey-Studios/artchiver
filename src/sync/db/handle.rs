use crate::shared::progress::ProgressMonitor;
use crate::shared::update::DataUpdate;
use crate::{
    shared::environment::Environment,
    sync::db::{
        tag::{DbTag, list_all_tags},
        writer::DbBgWriter,
    },
};
use anyhow::Result;
use artchiver_sdk::{Tag, Work};
use crossbeam::channel::{self, Receiver, Sender};
use log::info;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::collections::HashSet;
use std::{
    collections::HashMap,
    thread::{JoinHandle, spawn},
};

pub enum DbWriterRequest {
    UpsertTags { plugin_id: i64, tags: Vec<Tag> },
    UpsertWorks { works: Vec<Work> },
    SetWorkPreviewPath { work_id: i64, path: String },
    SetWorkScreenPath { work_id: i64, path: String },
    SetWorkArchivePath { work_id: i64, path: String },
    Exit,
}

#[derive(Clone, Debug)]
pub struct DbHandle {
    pool: r2d2::Pool<SqliteConnectionManager>,
    tx_to_app: Sender<DataUpdate>,
    tx_to_writer: Sender<DbWriterRequest>,
    // rx_app_from_threads: Receiver<DbResponse>,
}

pub struct DbThreads {
    writer_handle: JoinHandle<()>,
}

impl DbThreads {
    pub fn wait_for_exit(&mut self) {
        // self.writer_handle.join().expect("failed to wait on writer");
    }
}

pub fn connect_or_create(
    env: &Environment,
    progress_mon: &ProgressMonitor,
) -> Result<(DbHandle, DbThreads)> {
    info!(
        "Opening Metadata DB at {}",
        env.metadata_file_path().display()
    );
    let manager = SqliteConnectionManager::file(env.metadata_file_path())
        .with_init(|conn| rusqlite::vtab::array::load_module(conn));
    let pool = r2d2::Pool::builder().build(manager)?;
    let conn = pool.get()?;
    let params = [("journal_mode", "WAL", "wal")];
    for (name, value, expect) in params {
        info!("Configuring DB: {name} = {value}");
        let result: String =
            conn.query_one(&format!("PRAGMA {name} = {value};"), [], |row| row.get(0))?;
        assert_eq!(result, expect, "failed to configure database");
    }
    let params = [
        ("journal_size_limit", (64 * 1024 * 1024).to_string()),
        ("mmap_size", (1024 * 1024 * 1024).to_string()),
        ("busy_timeout", "5000".into()),
    ];
    for (name, value) in params {
        info!("Configuring DB: {name} = {value}");
        let _: i64 = conn.query_one(&format!("PRAGMA {name} = {value};"), [], |row| row.get(0))?;
    }
    let params = [("synchronous", "NORMAL"), ("cache_size", "2000")];
    for (name, value) in params {
        info!("Configuring DB: {name} = {value}");
        conn.execute(&format!("PRAGMA {name} = {value};"), [])?;
    }

    // List all migrations that we've already run.
    let finished_migrations = {
        match conn.prepare("SELECT ordinal FROM migrations") {
            Ok(mut stmt) => match stmt.query_map([], |row| row.get(0)) {
                Ok(q) => q.flatten().collect::<Vec<i64>>(),
                Err(_) => vec![],
            },
            Err(_) => vec![],
        }
    };

    // Execute and record all migration statements
    for (ordinal, migration) in crate::sync::db::model::MIGRATIONS.iter().enumerate() {
        if !finished_migrations.contains(&(ordinal as i64)) {
            conn.execute(migration, ())?;
            conn.execute("INSERT INTO migrations (ordinal) VALUES (?)", [ordinal])?;
        }
    }

    let (tx_to_writer, rx_writer_from_app) = channel::unbounded();
    let db_handle = DbHandle {
        pool: pool.clone(),
        tx_to_app: progress_mon.monitor_channel(),
        tx_to_writer,
    };
    let mut writer = DbBgWriter::new(pool, rx_writer_from_app, progress_mon.monitor_channel());
    let db_threads = DbThreads {
        writer_handle: spawn(move || if let Err(e) = writer.main() {}),
    };
    
    Ok((db_handle, db_threads))
}

impl DbHandle {
    // pub fn maintain_threads(&self) -> Vec<DataUpdate> {
    //     let mut out = Vec::new();
    //     while let Ok(msg) = self.rx_app_from_threads.try_recv() {
    //         out.push(match msg {
    //             DbResponse::InitialTags(tags) => DataUpdate::InitialTags(tags),
    //             DbResponse::TagsLocalCounts(counts) => DataUpdate::TagsLocalCounts(counts),
    //             DbResponse::TagsNetworkCounts(counts) => DataUpdate::TagsNetworkCounts(counts),
    //         });
    //     }
    //     out
    // }
    pub fn handle_updates(&mut self, _updates: &[DataUpdate]) {}

    // WRITE SIDE ////////////////////////////////////
    pub fn upsert_tags(&self, plugin_id: i64, tags: Vec<Tag>) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::UpsertTags { plugin_id, tags })?;
        Ok(())
    }

    pub fn upsert_works(&self, works: Vec<Work>) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::UpsertWorks { works })?;
        Ok(())
    }

    pub fn set_work_preview_path(&self, work_id: i64, path: String) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkPreviewPath { work_id, path })?;
        Ok(())
    }

    pub fn set_work_screen_path(&self, work_id: i64, path: String) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkScreenPath { work_id, path })?;
        Ok(())
    }

    pub fn set_work_archive_path(&self, work_id: i64, path: String) -> Result<()> {
        self.tx_to_writer
            .send(DbWriterRequest::SetWorkArchivePath { work_id, path })?;
        Ok(())
    }

    // READ SIDE /////////////////////////////////////
    pub fn get_tags(&self) {
        let conn = self.pool.get().expect("failed to get connection");
        let tx_to_app = self.tx_to_app.clone();
        rayon::spawn(move || {
            let tags = list_all_tags(&conn).expect("failed to list tags");
            let tags = tags.iter().map(|t| (t.id(), t.to_owned())).collect::<HashMap<_, _>>();
            tx_to_app.send(DataUpdate::InitialTags(tags)).unwrap();
        });
    }

    // SYSTEM ////////////////////////////////////////
    pub fn send_exit_request(&self) {
        self.tx_to_writer
            .send(DbWriterRequest::Exit)
            .expect("writer send died at exit");
    }

    // PLUGINS ///////////////////////////////////////
    pub fn sync_upsert_plugin(&self, plugin_name: &str) -> Result<i64> {
        let conn = self.pool.get()?;
        let row_cnt = conn.execute(
            "INSERT OR IGNORE INTO plugins (name) VALUES (?)",
            params![plugin_name],
        )?;
        let plugin_id = if row_cnt > 0 {
            conn.last_insert_rowid()
        } else {
            conn.query_row(
                "SELECT id FROM plugins WHERE name = ?",
                params![plugin_name],
                |row| row.get(0),
            )?
        };
        Ok(plugin_id)
    }

    pub fn sync_list_plugins_for_tag(&self, tag_id: i64) -> Result<HashSet<String>> {
        let conn = self.pool.get()?;
        let query = r#"SELECT DISTINCT p.name
            FROM plugins AS p
            INNER JOIN plugin_tags AS pt ON p.id = pt.plugin_id
            WHERE pt.tag_id = ?"#;
        Ok(conn
            .prepare(query)?
            .query_map([tag_id], |row| row.get(0))?
            .flatten()
            .collect())
    }

    // CONFIGURATION ///////////////////////////////////////
    pub fn sync_load_configurations(&self, plugin_id: i64) -> Result<Vec<(String, String)>> {
        let conn = self.pool.get()?;
        Ok(conn
            .prepare("SELECT key, value FROM plugin_configurations WHERE plugin_id = ?")?
            .query_map(params![plugin_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .flatten()
            .collect())
    }

    pub fn sync_save_configurations(
        &self,
        plugin_id: i64,
        configs: &[(String, String)],
    ) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("BEGIN TRANSACTION", ())?;
        for (k, v) in configs {
            conn.execute(
                r#"INSERT INTO plugin_configurations (plugin_id, key, value)
                        VALUES (?, ?, ?)
                        ON CONFLICT (plugin_id, key)
                        DO UPDATE SET value=?"#,
                params![plugin_id, k, v, v],
            )?;
        }
        conn.execute("COMMIT TRANSACTION", ())?;
        Ok(())
    }
}
