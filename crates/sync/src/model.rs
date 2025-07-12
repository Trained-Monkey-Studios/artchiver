use crate::{Environment, progress::ProgressSender};
use bevy::prelude::*;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::ops::Range;

const MIGRATIONS: [&str; 7] = [
    // Migrations
    r#"CREATE TABLE migrations (
        id INTEGER PRIMARY KEY,
        ordinal INTEGER NOT NULL UNIQUE
    );"#,
    // Plugins: Data sources; by name so that versions can change and the wasm file can move.
    r#"CREATE TABLE plugins (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        UNIQUE(name)
    );"#,
    // Tags: Attributes of a work, such as the author, subject-matter, etc.
    r#"CREATE TABLE tags (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        UNIQUE(name)
    );"#,
    // Works: A work of art
    r#"CREATE TABLE works (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        artist_id INTEGER NOT NULL,
        date TIMESTAMP,
        preview_url TEXT NOT NULL,
        screen_url TEXT NOT NULL,
        archive_url TEXT NOT NULL,
        FOREIGN KEY(artist_id) REFERENCES artists(id)
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
        UNIQUE(tag_id, work_id)
    );"#,
    // Plugin<->Tag: tag each tag with the plugin it came from, so we know
    //               what plugins to query for data about each tag.
    r#"CREATE TABLE plugin_tags (
        id INTEGER PRIMARY KEY,
        plugin_id INTEGER NOT NULL,
        tag_id INTEGER NOT NULL,
        FOREIGN KEY(plugin_id) REFERENCES plugins(id),
        FOREIGN KEY(tag_id) REFERENCES tags(id),
        UNIQUE(plugin_id, tag_id)
    );"#,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, SystemSet)]
pub enum MetadataSet {
    Connect,
}

pub struct MetadataPlugin;
impl Plugin for MetadataPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, connect_or_create_db.in_set(MetadataSet::Connect));
    }
}

fn connect_or_create_db(env: Res<Environment>, mut commands: Commands) -> Result {
    info!(
        "Opening Metadata DB at {}",
        env.metadata_file_path().display()
    );
    let manager = SqliteConnectionManager::file(env.metadata_file_path());
    let pool = r2d2::Pool::new(manager)?;
    let db = pool.get()?;

    // List all migrations that we've already run.
    let finished_migrations = {
        match db.prepare("SELECT ordinal FROM migrations") {
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
            db.execute(migration, ())?;
            db.execute("INSERT INTO migrations (ordinal) VALUES (?)", [ordinal])?;
        }
    }

    // Put our pool in a resource so everyone can use it.
    commands.insert_resource(MetadataPoolResource(MetadataPool::new(pool)));

    Ok(())
}

// Expose the metadata pool as a resource.
#[derive(Clone, Debug, Resource)]
pub struct MetadataPoolResource(MetadataPool);
impl MetadataPoolResource {
    pub fn pool(&self) -> MetadataPool {
        self.0.clone()
    }
}

// Wrap the low level connection details so that we cna provide a high level metadata model.
#[derive(Clone, Debug)]
pub struct MetadataPool {
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl MetadataPool {
    fn new(pool: r2d2::Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub fn upsert_plugin(&self, plugin_name: &str) -> Result<i64> {
        let pool = self.pool.get()?;
        let row_cnt = pool.execute(
            "INSERT OR IGNORE INTO plugins (name) VALUES (?)",
            params![plugin_name],
        )?;
        let plugin_id = if row_cnt > 0 {
            pool.last_insert_rowid()
        } else {
            pool.query_row(
                "SELECT id FROM plugins WHERE name = ?",
                params![plugin_name],
                |row| row.get(0),
            )?
        };
        Ok(plugin_id)
    }

    pub fn upsert_tags(
        &self,
        plugin_id: i64,
        tags: &[String],
        progress: &mut ProgressSender,
    ) -> Result {
        progress.set_spinner();
        let pool = self.pool.get()?;
        let mut insert_tag_stmt = pool.prepare("INSERT OR IGNORE INTO tags (name) VALUES (?)")?;
        let mut select_tag_id_stmt = pool.prepare("SELECT id FROM tags WHERE name = ?")?;
        let mut insert_plugin_tag_stmt =
            pool.prepare("INSERT OR IGNORE INTO plugin_tags (plugin_id, tag_id) VALUES (?, ?)")?;

        for (i, tag) in tags.iter().enumerate() {
            progress.set_percent(i, tags.len());
            let row_cnt = insert_tag_stmt.execute(params![tag])?;
            let tag_id = if row_cnt > 0 {
                progress.message(format!("Added tag '{tag}' to gallery {plugin_id}"));
                pool.last_insert_rowid()
            } else {
                progress.message(format!("Skipped adding existing tag '{tag}'"));
                select_tag_id_stmt.query_row(params![tag], |row| row.get(0))?
            };
            insert_plugin_tag_stmt.execute(params![plugin_id, tag_id])?;
        }
        progress.clear();
        Ok(())
    }

    pub fn count_tags(&self, filter: &str) -> Result<i64> {
        let pool = self.pool.get()?;
        let cnt = pool.query_row(
            "SELECT COUNT(*) FROM tags WHERE name LIKE ? ORDER BY name ASC",
            [format!("%{filter}%")],
            |row| row.get(0),
        )?;
        Ok(cnt)
    }

    pub fn list_tags(&self, range: Range<usize>, filter: &str) -> Result<Vec<String>> {
        let pool = self.pool.get()?;
        let mut stmt = pool.prepare(
            "SELECT name FROM tags WHERE name LIKE ? ORDER BY name ASC LIMIT ? OFFSET ?",
        )?;
        let rows = stmt.query_map(
            params![format!("%{filter}%"), range.end - range.start, range.start],
            |row| row.get(0),
        )?;
        Ok(rows.flatten().collect())
    }
}
