use jiff::civil::Date;
use rusqlite::Row;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct WorkId(i64);
impl WorkId {
    pub fn wrap(id: i64) -> Self {
        Self(id)
    }
}

// DB-centered [art]work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbWork {
    id: WorkId,
    name: String,
    artist_id: i64,
    date: Date,
    preview_url: String,
    screen_url: String,
    archive_url: Option<String>,

    preview_path: Option<PathBuf>,
    screen_path: Option<PathBuf>,
    archive_path: Option<PathBuf>,

    tags: Vec<String>,
}

impl DbWork {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let tag_str: String = row.get("tags")?;
        Ok(Self {
            id: WorkId(row.get("id")?),
            name: row.get("name")?,
            artist_id: row.get("artist_id")?,
            date: row.get("date")?,
            preview_url: row.get("preview_url")?,
            screen_url: row.get("screen_url")?,
            archive_url: row.get("archive_url")?,
            preview_path: row
                .get::<&str, Option<String>>("preview_path")?
                .map(|s| s.into()),
            screen_path: row
                .get::<&str, Option<String>>("screen_path")?
                .map(|s| s.into()),
            archive_path: row
                .get::<&str, Option<String>>("archive_path")?
                .map(|s| s.into()),
            tags: tag_str.split(',').map(|s| s.to_owned()).collect(),
        })
    }

    pub fn id(&self) -> WorkId {
        self.id
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn date(&self) -> &Date {
        &self.date
    }

    pub fn preview_url(&self) -> &str {
        self.preview_url.as_str()
    }

    pub fn screen_url(&self) -> &str {
        self.screen_url.as_str()
    }

    pub fn archive_url(&self) -> Option<&str> {
        self.archive_url.as_deref()
    }

    pub fn preview_path(&self) -> Option<&Path> {
        self.preview_path.as_deref()
    }

    pub fn screen_path(&self) -> Option<&Path> {
        self.screen_path.as_deref()
    }

    pub fn archive_path(&self) -> Option<&Path> {
        self.archive_path.as_deref()
    }

    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(|s| s.as_str())
    }

    // For updating inline in the UX when the UX gets a download ready notice.
    pub fn set_paths(
        &mut self,
        preview_path: PathBuf,
        screen_path: PathBuf,
        archive_path: Option<PathBuf>,
    ) {
        self.preview_path = Some(preview_path);
        self.screen_path = Some(screen_path);
        self.archive_path = archive_path;
    }
}
