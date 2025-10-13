use absolute_unit::prelude::*;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The kind of measurement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SiUnit {
    Gram,
    Meter,
}

/// An arbitrary physical characteristic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measurement {
    name: String,
    description: String,
    characteristics: HashMap<String, (f64, SiUnit)>,
}

impl Measurement {
    pub fn new<N: ToString, D: ToString>(name: N, description: D) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            characteristics: HashMap::new(),
        }
    }

    pub fn add_characteristic<N: ToString>(&mut self, name: N, value: f64, si_unit: SiUnit) {
        self.characteristics
            .insert(name.to_string(), (value, si_unit));
    }
}

/// Captures data about the physical nature of an artwork.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalData {
    // Describes the physical type of the object. Print, statue, photograph, etc.
    medium: String,

    // A formatted representation of the object's dimensions, for display
    dimensions_display: String,

    // The physical size of the object, if known.
    dimensions: Option<V3<Length<Meters>>>,

    // Set of arbitrary physical characteristics, if any.
    measurements: Vec<Measurement>,

    // How and where was the work inscribed with attribution
    inscription: String,

    // Identifying markings that can be used to identify a work with confidence.
    markings: String,

    // Additional identifying marks that are less obvious to the naked eye.
    watermarks: String,
}

/// Who created this work, where when, and in what circumstances. What has happened to it since.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct History {
    // Who created the work
    attribution: String,

    // Typically a "Last, First" version of attribution.
    attribution_sort_key: String,

    // A one-line description of the creation timeline.
    displaydate: String,

    // Extracted begin/end creation years, if known or relevant.
    beginyear: Option<i64>,
    endyear: Option<i64>,

    // TODO: TAGME?: 20 broad time categories for when an artwork was created.
    time_category: String, // 20 broad categories

    // What do we know about where this object has been?
    provenance: String,

    // Text acknowledging the source or origin of the artwork and the year the object was acquired by the museum.
    credit_line: String,
}

impl History {
    pub fn attribution(&self) -> &str {
        &self.attribution
    }
}

/// Where the work is currently located.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Location {
    // Name of the owner of the work.
    custody: Option<String>,

    // Name of the site where the artwork is located
    site: Option<String>,

    // Name of the room in which the artwork is displayed
    room: Option<String>,

    // Position in the room where the artwork is displayed
    position: Option<String>,

    // A description of the artwork in-situ in the exhibit
    description: Option<String>,

    // Whether the object is on display.
    on_display: Option<bool>,
}

impl Location {
    /// Add a custodian to the location; e.g. a gallery or curator.
    pub fn with_custody(mut self, custody: impl ToString) -> Self {
        self.custody = Some(custody.to_string());
        self
    }

    /// Add site information to the location; e.g. which building of a gallery is the work in.
    pub fn with_site(mut self, site: impl ToString) -> Self {
        self.site = Some(site.to_string());
        self
    }

    /// Add room information to the location; e.g. which room of the gallery is the work in.
    pub fn with_room(mut self, room: impl ToString) -> Self {
        self.room = Some(room.to_string());
        self
    }

    /// Add position information to the location; e.g. where is the work in the room.
    pub fn with_position(mut self, position: impl ToString) -> Self {
        self.position = Some(position.to_string());
        self
    }

    /// Add a description to the location; e.g. how does the work appear in the display.
    pub fn with_description(mut self, description: impl ToString) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Add a flag to the location to indicate if the object is currently on display.
    pub fn with_on_display(mut self, on_display: bool) -> Self {
        self.on_display = Some(on_display);
        self
    }

    /// The current custodian of the artwork; e.g. a gallery or curator.
    pub fn custody(&self) -> Option<&str> {
        self.custody.as_deref()
    }

    /// The current physical site that the work is displayed in; e.g. a gallery location.
    pub fn site(&self) -> Option<&str> {
        self.site.as_deref()
    }

    /// The room that the work is displayed in.
    pub fn room(&self) -> Option<&str> {
        self.room.as_deref()
    }

    /// The position in the room that the work is displayed in.
    pub fn position(&self) -> Option<&str> {
        self.position.as_deref()
    }

    /// A description of the artwork in-situ in the exhibit; e.g. to help with locating the work.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Indicates whether the work is currently generally viewable by the public.
    pub fn on_display(&self) -> Option<bool> {
        self.on_display
    }
}

/// API-centered \[art\]work item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    name: String,
    date: Date,
    preview_url: String,
    screen_url: String,
    tags: Vec<String>,

    remote_id: Option<String>,
    archive_url: Option<String>,

    physical_data: Option<PhysicalData>,
    history: Option<History>,
    location: Option<Location>,
}

impl Work {
    pub fn new<N: ToString, P: ToString, S: ToString>(
        name: N,
        date: Date,
        preview_url: P,
        screen_url: S,
        tags: Vec<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            date,
            preview_url: preview_url.to_string(),
            screen_url: screen_url.to_string(),
            tags,

            remote_id: None,
            archive_url: None,
            // artist_name: None,
            physical_data: None,
            history: None,
            location: None,
        }
    }

    pub fn with_remote_id(mut self, id: impl ToString) -> Self {
        self.remote_id = Some(id.to_string());
        self
    }

    pub fn with_archive_url(mut self, url: impl ToString) -> Self {
        self.archive_url = Some(url.to_string());
        self
    }

    pub fn with_physical_data(mut self, physical_data: PhysicalData) -> Self {
        self.physical_data = Some(physical_data);
        self
    }

    pub fn with_history(mut self, history: History) -> Self {
        self.history = Some(history);
        self
    }

    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn date(&self) -> &Date {
        &self.date
    }

    pub fn preview_url(&self) -> &str {
        &self.preview_url
    }

    pub fn screen_url(&self) -> &str {
        &self.screen_url
    }

    pub fn remote_id(&self) -> Option<&str> {
        self.remote_id.as_deref()
    }

    pub fn archive_url(&self) -> Option<&str> {
        self.archive_url.as_deref()
    }

    pub fn physical_data(&self) -> Option<&PhysicalData> {
        self.physical_data.as_ref()
    }

    pub fn history(&self) -> Option<&History> {
        self.history.as_ref()
    }

    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}
