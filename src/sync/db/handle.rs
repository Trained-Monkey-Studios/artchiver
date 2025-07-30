use crate::sync::db::tag::TagId;
use crate::{
    shared::{
        environment::Environment,
        progress::{HostUpdateSender, LogSender, ProgressMonitor, UpdateSource},
        update::DataUpdate,
    },
    sync::db::{
        plugin::{DbPlugin, PluginId},
        tag::{count_works_per_tag, list_all_tags},
        work::list_works_with_any_tags,
        writer::DbBgWriter,
    },
};
use anyhow::Result;
use artchiver_sdk::{Tag, Work};
use crossbeam::channel::{self, Sender};
use log::{debug, info, trace, warn};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::ThreadPool;
use rusqlite::params;
use std::time::{Duration, Instant};
use std::{
    collections::{HashMap, HashSet},
    thread::{JoinHandle, spawn},
};

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
pub struct DbHandle {
    pool: r2d2::Pool<SqliteConnectionManager>,
    tx_to_app: Sender<DataUpdate>,
    tx_to_writer: Sender<DbWriterRequest>,
}

pub struct DbThreads {
    db_pool: r2d2::Pool<SqliteConnectionManager>,
    writer_handle: JoinHandle<()>,
    reader_pool: ThreadPool,
    tx_to_app: Sender<DataUpdate>,
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

    let (tx_to_writer, rx_writer_from_app) = channel::unbounded();
    let db_handle = DbHandle {
        pool: pool.clone(),
        tx_to_app: progress_mon.monitor_channel(),
        tx_to_writer,
    };
    let reader_pool = rayon::ThreadPoolBuilder::new().build()?;
    let mut writer = DbBgWriter::new(
        pool.clone(),
        rx_writer_from_app,
        progress_mon.monitor_channel(),
    );
    let db_threads = DbThreads {
        db_pool: pool.clone(),
        writer_handle: spawn(move || if let Err(e) = writer.main() {}),
        reader_pool,
        tx_to_app: progress_mon.monitor_channel(),
    };

    Ok((db_handle, db_threads))
}

impl DbHandle {
    pub fn handle_updates(&mut self, _updates: &[DataUpdate]) {}

    pub fn send_exit_request(&self) {
        self.tx_to_writer
            .send(DbWriterRequest::Shutdown)
            .expect("writer send died at exit");
    }

    // WRITE SIDE ////////////////////////////////////
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
            .query_map([tag_id], |row| PluginId::from_row(row))?
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
}

impl DbThreads {
    pub fn get_tags(&self) {
        let conn = self.db_pool.get().expect("failed to get connection");
        let tx_to_app = self.tx_to_app.clone();
        self.reader_pool.spawn(move || {
            let tags = list_all_tags(&conn).expect("failed to list tags");
            let tags = tags
                .iter()
                .map(|t| (t.id(), t.to_owned()))
                .collect::<HashMap<_, _>>();
            trace!("Found {} tags", tags.len());
            tx_to_app.send(DataUpdate::InitialTags(tags)).unwrap();
            trace!("Dispatched initial tags to UX; getting counts");

            let work_counts = count_works_per_tag(&conn).expect("failed to count works per tag");
            tx_to_app
                .send(DataUpdate::TagsLocalCounts(work_counts))
                .expect("db reader disconnect");
            trace!("Dispatching tag local counts to UX");
        });
    }

    pub fn get_works(&self, tags: &[String]) {
        let mut host = HostUpdateSender::wrap(UpdateSource::DbReader, self.tx_to_app.clone());
        let mut log = LogSender::wrap(UpdateSource::DbReader, self.tx_to_app.clone());

        log.trace(format!("Fetching works for tags: {tags:?}"));
        let tags = tags.to_owned();
        let conn = self.db_pool.get().expect("failed to get connection");
        self.reader_pool.spawn(move || {
            let works = list_works_with_any_tags(&conn, &tags).expect("failed to list tags");
            let works = works
                .into_iter()
                .map(|w| (w.id(), w))
                .collect::<HashMap<_, _>>();
            log.trace(format!("Finished collecting {} works", works.len()));
            host.fetch_works_completed(works)
                .expect("connection closed");
        });
    }
}

pub fn report_slow_query(start: Instant, name: &str, query: &str) {
    let elapsed = start.elapsed();
    if elapsed > Duration::from_millis(30) {
        warn!("Slow query {name} took {elapsed:?}");
        debug!(
            "SQL:    {}",
            query.replace('\n', " ").replace("            ", " ")
        );
    }
}
