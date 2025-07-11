use crate::Environment;
use bevy::prelude::*;
use rusqlite::Connection;

const MIGRATIONS: [&str; 6] = [
    r#"
CREATE TABLE migrations (
    id INTEGER PRIMARY KEY,
    ordinal INTEGER NOT NULL UNIQUE
);
"#,
    r#"
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    UNIQUE(name)
);
"#,
    r#"
CREATE TABLE works (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
"#,
    r#"
CREATE TABLE work_tags (
    id INTEGER PRIMARY KEY,
    tag_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    FOREIGN KEY(tag_id) REFERENCES tags(id),
    FOREIGN KEY(work_id) REFERENCES works(id)
);
"#,
    r#"
CREATE TABLE artists (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    birthday TIMESTAMP,
    deathday TIMESTAMP,
    suffix TEXT,
    nationality TEXT,
    bio TEXT
);
"#,
    r#"
CREATE TABLE artist_works (
    id INTEGER PRIMARY KEY,
    artist_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    FOREIGN KEY(artist_id) REFERENCES artists(id),
    FOREIGN KEY(work_id) REFERENCES works(id)
);
"#,
];

pub(crate) fn connect_or_create_db(env: Res<Environment>) -> Result {
    let db_path = format!("{}/metadata.db", env.prefix().to_string_lossy());
    info!("Opening Metadata DB {db_path:?}");
    let db = Connection::open(db_path)?;

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

    Ok(())
}
