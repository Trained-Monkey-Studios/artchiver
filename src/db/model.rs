use log::{debug, warn};
use parking_lot::Mutex;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

pub const MIGRATIONS: [&str; 28] = [
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
        wiki_url TEXT,
        remote_id TEXT,
        hidden BOOLEAN NOT NULL DEFAULT false,
        favorite BOOLEAN NOT NULL DEFAULT false
    );"#,
    r#"CREATE UNIQUE INDEX tag_name_idx ON tags(name);"#,
    r#"CREATE INDEX tag_favorite_idx ON tags(favorite);"#,
    r#"CREATE INDEX tag_hidden_idx ON tags(hidden);"#,
    // Works: A work of art
    r#"CREATE TABLE works (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        artist_id INTEGER NOT NULL,
        date TIMESTAMP,
        favorite BOOLEAN NOT NULL DEFAULT false,
        hidden BOOLEAN NOT NULL DEFAULT false,
        preview_url TEXT NOT NULL,
        screen_url TEXT NOT NULL UNIQUE,
        archive_url TEXT,
        preview_path TEXT,
        screen_path TEXT,
        archive_path TEXT
    );"#,
    // TODO: FOREIGN KEY(artist_id) REFERENCES artists(id)
    r#"CREATE UNIQUE INDEX work_screen_url_idx ON works(screen_url);"#,
    r#"CREATE INDEX work_name_idx ON works(name);"#,
    r#"CREATE INDEX work_date_idx ON works(date);"#,
    r#"CREATE INDEX work_id_date_idx ON works(id, date);"#,
    r#"CREATE INDEX work_favorite_idx ON works(favorite);"#,
    r#"CREATE INDEX work_hidden_idx ON works(hidden);"#,
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
    r#"CREATE INDEX work_tags_tag_idx ON work_tags(tag_id);"#,
    r#"CREATE INDEX work_tags_work_idx ON work_tags(work_id);"#,
    // Plugin<->Tag: tag each tag with the plugin it came from, so we know
    //               what plugins to query for data about each tag.
    r#"CREATE TABLE plugin_tags (
        id INTEGER PRIMARY KEY,
        plugin_id INTEGER NOT NULL,
        tag_id INTEGER NOT NULL,
        presumed_work_count INTEGER,
        FOREIGN KEY(plugin_id) REFERENCES plugins(id),
        FOREIGN KEY(tag_id) REFERENCES tags(id),
        UNIQUE (plugin_id, tag_id)
    );"#,
    r#"CREATE INDEX plugin_tags_tag_idx ON plugin_tags(tag_id);"#,
    r#"CREATE INDEX plugin_tags_plugin_idx ON plugin_tags(plugin_id);"#,
    r#"CREATE INDEX plugin_tags_work_count_idx ON plugin_tags(presumed_work_count);"#,
    // Expand works information
    r#"ALTER TABLE works ADD COLUMN location_custody TEXT"#,
    r#"ALTER TABLE works ADD COLUMN location_site TEXT"#,
    r#"ALTER TABLE works ADD COLUMN location_room TEXT"#,
    r#"ALTER TABLE works ADD COLUMN location_position TEXT"#,
    r#"ALTER TABLE works ADD COLUMN location_description TEXT"#,
    r#"ALTER TABLE works ADD COLUMN location_on_display BOOLEAN"#,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum OrderDir {
    #[default]
    Asc,
    Desc,
}

impl fmt::Display for OrderDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        };
        write!(f, "{s}")
    }
}

impl OrderDir {
    pub fn ui(&mut self, salt: &str, ui: &mut egui::Ui) {
        let mut selected = match self {
            Self::Asc => 0,
            Self::Desc => 1,
        };
        let options = ["Ascending", "Descending"];
        egui::ComboBox::new(format!("order_dir_{salt}"), "")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, options.len(), |i| options[i]);
        *self = match selected {
            0 => Self::Asc,
            1 => Self::Desc,
            _ => panic!("invalid column selected"),
        };
    }
}

#[derive(Clone, Debug, Default)]
pub struct DbCancellation {
    cancelled: Arc<Mutex<bool>>,
}

impl DbCancellation {
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.lock()
    }

    pub fn cancel(&self) {
        *self.cancelled.lock() = true;
    }

    pub fn reset(&self) {
        *self.cancelled.lock() = false;
    }
}

pub fn string_to_rarray(v: &[String]) -> Rc<Vec<Value>> {
    Rc::new(v.iter().cloned().map(Value::from).collect())
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
