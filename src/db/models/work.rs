use crate::db::models::tag::TagId;
use anyhow::anyhow;
use artchiver_sdk::{History, Location, Measurement, PhysicalData, SiUnit};
use jiff::civil::Date;
use rusqlite::types::{ToSqlOutput, Value};
use rusqlite::{Row, ToSql};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use itertools::Itertools;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct WorkId(i64);
impl ToSql for WorkId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(self.0)))
    }
}
impl WorkId {
    pub fn wrap(id: i64) -> Self {
        Self(id)
    }
}

pub fn location_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Location>> {
    let mut loc = Location::default();
    if let Some(custody) = row.get::<&str, Option<String>>("location_custody")? {
        loc = loc.with_custody(custody);
    }
    if let Some(site) = row.get::<&str, Option<String>>("location_site")? {
        loc = loc.with_site(site);
    }
    if let Some(room) = row.get::<&str, Option<String>>("location_room")? {
        loc = loc.with_room(room);
    }
    if let Some(position) = row.get::<&str, Option<String>>("location_position")? {
        loc = loc.with_position(position);
    }
    if let Some(description) = row.get::<&str, Option<String>>("location_description")? {
        loc = loc.with_description(description);
    }
    if let Some(on_display) = row.get::<&str, Option<bool>>("location_on_display")? {
        loc = loc.with_on_display(on_display);
    }
    if loc == Location::default() {
        return Ok(None);
    }
    Ok(Some(loc))
}

pub fn history_from_row(row: &Row<'_>) -> rusqlite::Result<Option<History>> {
    let mut history = History::default();
    if let Some(v) = row.get::<&str, Option<String>>("history_attribution")? {
        history.set_attribution(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("history_attribution_sort_key")? {
        history.set_attribution_sort_key(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("history_display_date")? {
        history.set_display_date(v);
    }
    if let Some(v) = row.get::<&str, Option<i64>>("history_begin_year")? {
        history.set_begin_year(v);
    }
    if let Some(v) = row.get::<&str, Option<i64>>("history_end_year")? {
        history.set_end_year(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("history_provenance")? {
        history.set_provenance(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("history_credit_line")? {
        history.set_credit_line(v);
    }
    if history == History::default() {
        return Ok(None);
    }
    Ok(Some(history))
}

pub fn physical_from_row(row: &Row<'_>) -> rusqlite::Result<Option<PhysicalData>> {
    let mut physical = PhysicalData::default();
    if let Some(v) = row.get::<&str, Option<String>>("physical_medium")? {
        physical.set_medium(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("physical_dimensions_display")? {
        physical.set_dimensions_display(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("physical_inscription")? {
        physical.set_inscription(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("physical_markings")? {
        physical.set_markings(v);
    }
    if let Some(v) = row.get::<&str, Option<String>>("physical_watermarks")? {
        physical.set_watermarks(v);
    }
    // FIXME: need the measurements list
    if physical == PhysicalData::default() {
        return Ok(None);
    }
    Ok(Some(physical))
}

pub fn measurements_from_row(row: &Row<'_>) -> rusqlite::Result<Vec<Measurement>> {
    let mut measurements = Vec::new();
    let measures_str: String = row.get("measure_names").ok().unwrap_or_default();
    for measure_item in measures_str.split(',') {
        if measure_item.trim().is_empty() {
            continue;
        }
        let mut parts = measure_item.split('|');
        let name = parts.next().expect("measure name");
        let desc = parts.next().expect("measure description");
        let value = parts.next().expect("measure value");
        let unit = parts.next().expect("measure unit");
        let mut measurement = Measurement::new(
            value.parse::<f64>().map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(
                    anyhow!("failed to parse measure value: {value}: {e}").into(),
                )
            })?,
            SiUnit::try_from(unit).map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(
                    anyhow!("unexpected measure unit: {unit}: {e}").into(),
                )
            })?,
        )
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(
                anyhow!("invalid measure value: {value}: {e}").into(),
            )
        })?;
        if !name.is_empty() {
            measurement.set_name(name);
        }
        if !desc.is_empty() {
            measurement.set_description(desc);
        }
        measurements.push(measurement);
    }
    Ok(measurements)
}

// DB-centered [art]work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbWork {
    id: WorkId,
    name: String,
    artist_id: i64,
    date: Date,

    favorite: bool,
    hidden: bool,

    location: Option<Location>,
    history: Option<History>,
    physical_data: Option<PhysicalData>,

    preview_url: String,
    screen_url: String,
    archive_url: Option<String>,

    preview_path: Option<PathBuf>,
    screen_path: Option<PathBuf>,
    archive_path: Option<PathBuf>,

    tags: Vec<TagId>,
}

impl DbWork {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let tag_str: String = row.get("tags").ok().unwrap_or_default();
        let tags = tag_str
            .split(',')
            .map(|s| TagId::wrap(s.parse::<i64>().expect("valid ids")))
            .collect();

        let measurements = measurements_from_row(row)?;

        Ok(Self {
            id: WorkId(row.get("id")?),
            name: row.get("name")?,
            artist_id: row.get("artist_id")?,
            date: row.get("date")?,
            favorite: row.get("favorite")?,
            hidden: row.get("hidden")?,
            location: location_from_row(row)?,
            history: history_from_row(row)?,
            physical_data: physical_from_row(row)?
                .map(|physical| physical.with_measurements(measurements.into_iter())),
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
            tags,
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

    pub fn favorite(&self) -> bool {
        self.favorite
    }

    pub fn favorite_annotation(&self) -> &'static str {
        if self.favorite { "✨" } else { "" }
    }

    pub fn set_favorite(&mut self, favorite: bool) {
        self.favorite = favorite;
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    pub fn history(&self) -> Option<&History> {
        self.history.as_ref()
    }

    pub fn physical_data(&self) -> Option<&PhysicalData> {
        self.physical_data.as_ref()
    }

    pub fn screen_path(&self) -> Option<&Path> {
        self.screen_path.as_deref()
    }

    pub fn archive_path(&self) -> Option<&Path> {
        self.archive_path.as_deref()
    }

    pub fn tags(&self) -> impl Iterator<Item = TagId> {
        self.tags.iter().copied()
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
