use crate::shared::{environment::Environment, progress::ProgressSender, tag::TagSet};
use anyhow::Result;
use artchiver_sdk::Work;
use log::info;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, types::Value};
use std::{ops::Range, rc::Rc};

const MIGRATIONS: [&str; 9] = [
    // Migrations
    r#"CREATE TABLE migrations (
        id INTEGER PRIMARY KEY,
        ordinal INTEGER NOT NULL UNIQUE
    );"#,
    r#"CREATE TABLE plugin_configurations (
        id INTEGER PRIMARY KEY,
        plugin_id INTEGER NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        FOREIGN KEY(plugin_id) REFERENCES plugins(id),
        UNIQUE (plugin_id, key)
    );"#,
    // Plugins: Data sources; by name so that versions can change and the wasm file can move.
    r#"CREATE TABLE plugins (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE
    );"#,
    // Tags: Attributes of a work, such as the author, subject-matter, etc.
    r#"CREATE TABLE tags (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        kind TEXT DEFAULT 'default',
        presumed_work_count INTEGER,
        hidden BOOLEAN NOT NULL DEFAULT false,
        favorite BOOLEAN NOT NULL DEFAULT false
    );"#,
    r#"CREATE UNIQUE INDEX tag_name_idx ON tags(name);"#,
    // Works: A work of art
    r#"CREATE TABLE works (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        artist_id INTEGER NOT NULL,
        date TIMESTAMP,
        preview_url TEXT NOT NULL,
        screen_url TEXT NOT NULL UNIQUE,
        archive_url TEXT
        -- TODO: FOREIGN KEY(artist_id) REFERENCES artists(id)
    );"#,
    // Artists: The creator of a work of art
    r#"CREATE TABLE artists (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        birthday TIMESTAMP,
        deathday TIMESTAMP,
        suffix TEXT,
        nationality TEXT,
        bio TEXT
    );"#,
    // Work<->Tag: Associate a work with the tags that describe it and map a
    //             tag to the works with that content.
    r#"CREATE TABLE work_tags (
        id INTEGER PRIMARY KEY,
        tag_id INTEGER NOT NULL,
        work_id INTEGER NOT NULL,
        FOREIGN KEY(tag_id) REFERENCES tags(id),
        FOREIGN KEY(work_id) REFERENCES works(id),
        UNIQUE (tag_id, work_id)
    );"#,
    // Plugin<->Tag: tag each tag with the plugin it came from, so we know
    //               what plugins to query for data about each tag.
    r#"CREATE TABLE plugin_tags (
        id INTEGER PRIMARY KEY,
        plugin_id INTEGER NOT NULL,
        tag_id INTEGER NOT NULL,
        FOREIGN KEY(plugin_id) REFERENCES plugins(id),
        FOREIGN KEY(tag_id) REFERENCES tags(id),
        UNIQUE (plugin_id, tag_id)
    );"#,
];

fn string_to_rarray(v: &[String]) -> Rc<Vec<Value>> {
    Rc::new(v.iter().cloned().map(Value::from).collect())
}

// Wrap the low level connection details so that we cna provide a high level metadata model.
#[derive(Clone, Debug)]
pub struct MetadataPool {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl MetadataPool {
    pub fn connect_or_create(env: &Environment) -> Result<Self> {
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
            let _: i64 =
                conn.query_one(&format!("PRAGMA {name} = {value};"), [], |row| row.get(0))?;
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
        for (ordinal, migration) in MIGRATIONS.iter().enumerate() {
            if !finished_migrations.contains(&(ordinal as i64)) {
                conn.execute(migration, ())?;
                conn.execute("INSERT INTO migrations (ordinal) VALUES (?)", [ordinal])?;
            }
        }

        Ok(Self { pool })
    }

    pub fn get(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }

    pub fn load_configurations(&self, plugin_id: i64) -> Result<Vec<(String, String)>> {
        let conn = self.pool.get()?;
        Ok(conn
            .prepare("SELECT key, value FROM plugin_configurations WHERE plugin_id = ?")?
            .query_map(params![plugin_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .flatten()
            .collect())
    }

    pub fn save_configurations(&self, plugin_id: i64, configs: &[(String, String)]) -> Result<()> {
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

    pub fn upsert_plugin(&self, plugin_name: &str) -> Result<i64> {
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

    pub fn upsert_works(&mut self, works: &[Work], progress: &mut ProgressSender) -> Result<()> {
        let conn = self.pool.get()?;
        let mut insert_work_stmt = conn.prepare("INSERT OR IGNORE INTO works (name, artist_id, date, preview_url, screen_url, archive_url) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")?;
        let mut select_tag_ids_from_names =
            conn.prepare("SELECT id FROM tags WHERE name IN rarray(?)")?;
        let mut insert_work_tag_stmt =
            conn.prepare("INSERT OR IGNORE INTO work_tags (tag_id, work_id) VALUES (?, ?)")?;
        let mut select_work_id_stmt = conn.prepare("SELECT id FROM works WHERE name = ?")?;

        let total_count = works.len();
        let mut current_pos = 0;
        progress.info(format!("Writing {total_count} works to the database..."));

        for chunk in works.chunks(10_000) {
            progress.trace(format!("db->upsert_works chunk of {}", chunk.len()));
            conn.execute("BEGIN TRANSACTION", ())?;
            for work in chunk {
                let work_id = if let Ok(work_id) = insert_work_stmt.query_one(
                    params![
                        work.name(),
                        work.artist_id(),
                        work.date(),
                        work.preview_url(),
                        work.screen_url(),
                        work.archive_url()
                    ],
                    |row| row.get::<usize, i64>(0),
                ) {
                    work_id
                } else {
                    progress.trace(format!(
                        "db->upsert_works work {} already exists",
                        work.name()
                    ));
                    if let Ok(work_id) =
                        select_work_id_stmt.query_row(params![work.name()], |row| row.get(0))
                    {
                        work_id
                    } else {
                        progress.warn(format!(
                            "Detected duplicate URL in work {}, skipping",
                            work.name()
                        ));
                        continue;
                    }
                };
                let tag_ids: Vec<i64> = select_tag_ids_from_names
                    .query_map([string_to_rarray(work.tags())], |row| row.get(0))?
                    .flatten()
                    .collect();
                for tag_id in &tag_ids {
                    insert_work_tag_stmt.execute(params![*tag_id, work_id])?;
                }
            }
            conn.execute("COMMIT TRANSACTION", ())?;

            current_pos += chunk.len();
            progress.set_percent(current_pos, total_count);
            progress.database_changed()?;
        }

        Ok(())
    }

    fn make_works_query(order: &str, postfix: &str) -> String {
        format!(
            r#"SELECT works.id, works.name, works.artist_id, works.date, works.preview_url, works.screen_url, works.archive_url
            FROM works
            LEFT JOIN work_tags ON work_tags.work_id = works.id
            LEFT JOIN tags ON tags.id = work_tags.tag_id
            WHERE tags.name in rarray(?)
            GROUP BY works.id HAVING COUNT(DISTINCT tags.name) = ?
            {order}
            {postfix}"#
        )
    }

    pub fn works_count(&self, tag_set: &TagSet) -> Result<i64> {
        if tag_set.enabled_count() == 0 {
            return Ok(0);
        }
        let conn = self.pool.get()?;
        let count: i64 = conn.query_one(
            &format!("SELECT COUNT(*) FROM ({});", Self::make_works_query("", "")),
            params![tag_set.enabled_rarray(), tag_set.enabled_count()],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn works_list(&self, range: Range<usize>, tag_set: &TagSet) -> Result<Vec<Work>> {
        let conn = self.pool.get()?;
        let enabled_count = tag_set.enabled_count();
        if enabled_count == 0 {
            return Ok(vec![]);
        }
        let mut stmt = conn.prepare(&Self::make_works_query(
            "ORDER BY works.date ASC",
            "LIMIT ? OFFSET ?;",
        ))?;
        let works: Vec<Work> = stmt
            .query_map(
                params![
                    tag_set.enabled_rarray(),
                    enabled_count,
                    range.end - range.start,
                    range.start
                ],
                |row| {
                    Ok(Work::new(
                        row.get::<usize, String>(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        None,
                        vec![],
                    )
                    .with_id(row.get(0)?))
                },
            )?
            .flatten()
            .collect();
        Ok(works)
    }

    pub fn lookup_work(&self, work_id: i64) -> Result<Work> {
        let conn = self.pool.get()?;

        let work = conn.query_one(
            r#"SELECT id, name, artist_id, date, preview_url, screen_url, archive_url
                              FROM works WHERE id = ?"#,
            [work_id],
            |row| {
                Ok(Work::new(
                    row.get::<usize, String>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    None,
                    vec![],
                )
                .with_id(row.get(0)?))
            },
        )?;
        let tags = conn.prepare(
            r#"SELECT tags.name FROM tags LEFT JOIN work_tags ON work_tags.tag_id = tags.id WHERE work_tags.work_id = ?"#)?
            .query_map([work_id], |row| row.get(0))?.flatten().collect();
        Ok(work.with_tags(tags))
    }
}
