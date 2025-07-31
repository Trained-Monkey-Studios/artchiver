use crate::sync::db::work::WorkId;
use crate::{
    shared::{environment::Environment, progress::ProgressMonitor},
    sync::db::{
        plugin::{DbPlugin, PluginId},
        reader::DbReadHandle,
        tag::TagId,
        writer::{DbBgWriter, DbWriteHandle},
    },
};
use anyhow::Result;
use crossbeam::channel;
use log::{error, info};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::{collections::HashSet, thread};

pub fn connect_or_create(
    env: &Environment,
    progress_mon: &ProgressMonitor,
) -> Result<(DbSyncHandle, DbWriteHandle, DbReadHandle)> {
    info!(
        "Opening Metadata DB at {}",
        env.metadata_file_path().display()
    );
    let manager = SqliteConnectionManager::file(env.metadata_file_path())
        .with_init(|conn| rusqlite::vtab::array::load_module(conn));
    let pool = r2d2::Pool::builder().max_size(32).build(manager)?;
    let conn = pool.get()?;
    // FIXME: use library intrinsics to set these rather than `execute`
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

    // Send writes to a background thread.
    let (tx_to_writer, rx_writer_from_app) = channel::unbounded();
    let mut writer = DbBgWriter::new(
        pool.clone(),
        rx_writer_from_app,
        progress_mon.monitor_channel(),
    );
    let writer_handle = thread::spawn(move || {
        if let Err(e) = writer.main() {
            error!("Error in DB writer thread: {e}");
            panic!("Error in DB writer thread: {e}")
        }
    });
    let db_writer = DbWriteHandle::new(tx_to_writer);

    let reader_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .thread_name(|_| "DB Read Pool".to_owned())
        .build()?;
    let db_reader = DbReadHandle::new(
        pool.clone(),
        reader_pool,
        progress_mon.monitor_channel(),
        writer_handle,
    );

    let db_sync = DbSyncHandle { pool: pool.clone() };

    Ok((db_sync, db_writer, db_reader))
}

#[derive(Clone, Debug)]
pub struct DbSyncHandle {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl DbSyncHandle {
    // PLUGINS ///////////////////////////////////////
    pub fn sync_upsert_plugin(&self, plugin_name: &str) -> Result<DbPlugin> {
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
        let configs: Vec<(String, String)> = conn
            .prepare("SELECT key, value FROM plugin_configurations WHERE plugin_id = ?")?
            .query_map(params![plugin_id], |row| {
                Ok((row.get("key")?, row.get("value")?))
            })?
            .flatten()
            .collect();
        Ok(DbPlugin::new(plugin_id, plugin_name.to_owned(), configs))
    }

    pub fn sync_list_plugins_for_tag(&self, tag_id: TagId) -> Result<HashSet<PluginId>> {
        let conn = self.pool.get()?;
        let query = r#"SELECT DISTINCT p.id
            FROM plugins AS p
            INNER JOIN plugin_tags AS pt ON p.id = pt.plugin_id
            WHERE pt.tag_id = ?"#;
        Ok(conn
            .prepare(query)?
            .query_map([tag_id], PluginId::from_row)?
            .flatten()
            .collect())
    }

    // CONFIGURATION ///////////////////////////////////////
    pub fn sync_save_configurations(
        &self,
        plugin_id: PluginId,
        configs: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.pool.get()?;
        let xaction = conn.transaction()?;
        for (k, v) in configs {
            xaction.execute(
                r#"INSERT INTO plugin_configurations (plugin_id, key, value)
                        VALUES (?, ?, ?)
                        ON CONFLICT (plugin_id, key)
                        DO UPDATE SET value=?"#,
                params![plugin_id, k, v, v],
            )?;
        }
        xaction.commit()?;
        Ok(())
    }

    // WORK POKE /////////////////////////////////////////
    pub fn set_work_favorite(&self, work_id: WorkId, favorite: bool) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE works SET favorite = ? WHERE id = ?",
            params![favorite, work_id],
        )?;
        Ok(())
    }

    pub fn set_work_hidden(&self, work_id: WorkId, hidden: bool) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE works SET hidden = ? WHERE id = ?",
            params![hidden, work_id],
        )?;
        Ok(())
    }
}
