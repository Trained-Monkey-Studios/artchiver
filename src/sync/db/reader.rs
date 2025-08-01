use crate::{
    shared::{
        progress::{HostUpdateSender, LogSender, UpdateSource},
        update::DataUpdate,
    },
    sync::db::{
        model::report_slow_query,
        tag::{DbTag, TagId},
        work::DbWork,
    },
};
use anyhow::Result;
use crossbeam::channel::Sender;
use log::trace;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rayon::ThreadPool;
use std::{collections::HashMap, mem, thread, thread::JoinHandle, time::Instant};

#[derive(Debug)]
pub struct DbReadHandle {
    pool: r2d2::Pool<SqliteConnectionManager>,
    reader_threads: ThreadPool,

    log: LogSender,
    host: HostUpdateSender,

    // Note: the read handle is explicitly not clone because it also owns the threads. Yes, even
    //       the write thread. This is weird, but is purely for practical reasons as the reader
    //       never needs to leave the main UX thread.
    writer_handle: JoinHandle<()>,
}

impl DbReadHandle {
    pub fn new(
        pool: r2d2::Pool<SqliteConnectionManager>,
        reader_threads: ThreadPool,
        tx_to_app: Sender<DataUpdate>,
        writer_handle: JoinHandle<()>,
    ) -> Self {
        Self {
            pool,
            reader_threads,

            log: LogSender::wrap(UpdateSource::DbReader, tx_to_app.clone()),
            host: HostUpdateSender::wrap(UpdateSource::DbReader, tx_to_app),

            writer_handle,
        }
    }

    pub fn wait_for_exit(&mut self) {
        // We can't safely steal the join handle here because egui's on_shutdown message gives
        // us an &mut self, presumably because it has more work to do. If we were to steal the
        // handle, we would leave the handle in an un-init state and since the rest of the state
        // isn't dead, this would be a safety violation. To ensure that there is valid, initialized
        // memory in the handle, we swap in a new thread's handle for a thread that will immediately
        // terminate, allowing us to join the live DB-owning handle. This allows it the time it
        // needs to safely unwind and close the DB pool.
        let mut handle = thread::spawn(|| {});
        mem::swap(&mut self.writer_handle, &mut handle);
        handle.join().expect("db writer thread panicked");
    }

    pub fn get_tags(&self) {
        let conn = self.pool.get().expect("failed to get connection");
        let mut host = self.host.clone();
        self.reader_threads.spawn(move || {
            let tags = list_all_tags(&conn).expect("failed to list tags");
            let tags = tags
                .iter()
                .map(|t| (t.id(), t.to_owned()))
                .collect::<HashMap<_, _>>();
            trace!("Found {} tags", tags.len());
            host.fetch_tags_initial_complete(tags)
                .expect("db reader disconnect");
            trace!("Dispatched initial tags to UX; getting counts");

            let work_counts = count_works_per_tag(&conn).expect("failed to count works per tag");
            host.fetch_tags_local_counts_complete(work_counts)
                .expect("db reader disconnect");
            trace!("Dispatching tag local counts to UX");
        });
    }

    pub fn get_tag_local_counts(&self) {
        let conn = self.pool.get().expect("failed to get connection");
        let mut host = self.host.clone();
        self.reader_threads.spawn(move || {
            let work_counts = count_works_per_tag(&conn).expect("failed to count works per tag");
            host.fetch_tags_local_counts_complete(work_counts)
                .expect("db reader disconnect");
            trace!("Dispatching tag local counts to UX");
        });
    }

    pub fn get_works_for_tag(&self, tag_id: TagId) {
        let mut log = self.log.clone();
        let mut host = self.host.clone();
        log.trace(format!("Fetching works for tag: {tag_id:?}"));
        let conn = self.pool.get().expect("failed to get connection");
        self.reader_threads.spawn(move || {
            let works = list_works_with_tag(&conn, tag_id).expect("failed to list works");
            let works = works
                .into_iter()
                .map(|w| (w.id(), w))
                .collect::<HashMap<_, _>>();
            log.trace(format!("Finished collecting {} works", works.len()));
            host.fetch_works_completed(works)
                .expect("connection closed");
            trace!("Dispatching fetched works to UX");
        });
    }

    pub fn get_favorite_works(&self) {
        let mut log = self.log.clone();
        let mut host = self.host.clone();
        log.trace("Fetching favorite works");
        let conn = self.pool.get().expect("failed to get connection");
        self.reader_threads.spawn(move || {
            let works = list_favorite_works(&conn).expect("failed to list favorites");
            let works = works
                .into_iter()
                .map(|w| (w.id(), w))
                .collect::<HashMap<_, _>>();
            log.trace(format!("Finished collecting {} works", works.len()));
            host.fetch_works_completed(works)
                .expect("connection closed");
            trace!("Dispatching fetched works to UX");
        });
    }
}

pub fn list_works_with_tag(
    conn: &PooledConnection<SqliteConnectionManager>,
    tag_id: TagId,
) -> Result<Vec<DbWork>> {
    let start = Instant::now();

    // TODO: stream back incrementally?
    // If we decide we *have* to apply AND up front, it looks like this.
    // GROUP BY works.id HAVING COUNT(DISTINCT tags.name) = {enabled_size}
    let query = r#"
        SELECT works.*, GROUP_CONCAT(tags.id) as tags FROM works
        LEFT JOIN work_tags ON work_tags.work_id = works.id
        LEFT JOIN tags ON work_tags.tag_id = tags.id
        WHERE works.id IN (
            SELECT works.id FROM works
            LEFT JOIN work_tags ON work_tags.work_id = works.id
            LEFT JOIN tags ON work_tags.tag_id = tags.id
            WHERE tags.id = ?
        )
        GROUP BY works.id
    "#;
    let mut stmt = conn.prepare(query)?;
    let out = stmt.query_map([tag_id], DbWork::from_row)?.try_fold(
        Vec::new(),
        |mut expand, item| -> Result<Vec<DbWork>> {
            expand.push(item?);
            Ok(expand)
        },
    )?;
    report_slow_query(start, "list_works_with_any_tags", query);
    Ok(out)
}

pub fn list_favorite_works(
    conn: &PooledConnection<SqliteConnectionManager>,
) -> Result<Vec<DbWork>> {
    let start = Instant::now();
    let query = r#"
        SELECT works.*, GROUP_CONCAT(tags.id) as tags FROM works
        LEFT JOIN work_tags ON work_tags.work_id = works.id
        LEFT JOIN tags ON work_tags.tag_id = tags.id
        WHERE works.id IN (
            SELECT works.id FROM works WHERE works.favorite = 1
        )
        GROUP BY works.id
    "#;
    let mut stmt = conn.prepare(query)?;
    let out = stmt.query_map((), DbWork::from_row)?.try_fold(
        Vec::new(),
        |mut expand, item| -> Result<Vec<DbWork>> {
            expand.push(item?);
            Ok(expand)
        },
    )?;
    report_slow_query(start, "list_favorite_works", query);
    Ok(out)
}

pub fn list_all_tags(conn: &PooledConnection<SqliteConnectionManager>) -> Result<Vec<DbTag>> {
    let query = r#"
    SELECT tags.id, tags.name, tags.kind, tags.wiki_url, tags.favorite,
            SUM(plugin_tags.presumed_work_count) AS network_count,
            GROUP_CONCAT(plugins.name) AS plugin_names
        FROM tags
        LEFT JOIN plugin_tags ON tags.id == plugin_tags.tag_id
        LEFT JOIN plugins ON plugin_tags.plugin_id == plugins.id
        WHERE tags.hidden = 0
        GROUP BY tags.name, plugin_tags.presumed_work_count;"#;
    let mut stmt = conn.prepare(query)?;
    let tags: Vec<DbTag> = stmt.query_map((), DbTag::from_row)?.flatten().collect();
    Ok(tags)
}

pub fn count_works_per_tag(
    conn: &PooledConnection<SqliteConnectionManager>,
) -> Result<Vec<(TagId, u64)>> {
    let query = r#"SELECT tags.id, COUNT(work_tags.id)
        FROM tags
        LEFT JOIN work_tags ON tags.id == work_tags.tag_id
        WHERE tags.hidden = 0
        GROUP BY tags.id;"#;
    let out = conn
        .prepare(query)?
        .query_map((), |row| {
            let tag_id = TagId::wrap(row.get(0)?);
            let count = row.get(1)?;
            Ok((tag_id, count))
        })?
        .flatten()
        .collect();
    Ok(out)
}
