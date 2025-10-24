use absolute_unit::prelude::*;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

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
    value: f64,
    si_unit: SiUnit,
}

impl Measurement {
    pub fn new<N: ToString, D: ToString>(
        name: N,
        description: D,
        value: f64,
        si_unit: SiUnit,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            value,
            si_unit,
        }
    }
}

/// Captures data about the physical nature of an artwork.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PhysicalData {
    // Describes the physical type of the object. Print, statue, photograph, etc.
    medium: Option<String>,

    // A formatted representation of the object's dimensions, for display
    dimensions_display: Option<String>,

    // The physical size of the object, if known.
    dimensions: Option<V3<Length<Meters>>>,

    // Set of arbitrary physical characteristics, if any.
    measurements: Vec<Measurement>,

    // How and where was the work inscribed with attribution
    inscription: Option<String>,

    // Identifying markings that can be used to identify a work with confidence.
    markings: Option<String>,

    // Additional identifying marks that are less obvious to the naked eye.
    watermarks: Option<String>,
}

impl PhysicalData {
    // -- Getters --

    /// Describes the physical type of the object. Print, statue, photograph, etc.
    pub fn medium(&self) -> Option<&str> {
        self.medium.as_deref()
    }

    /// A formatted representation of the object's dimensions, for display, for cases where simple
    /// volumetric dimensions or measurements are not appropriate.
    pub fn dimensions_display(&self) -> Option<&str> {
        self.dimensions_display.as_deref()
    }

    /// The physical extents of the object, if known.
    pub fn dimensions(&self) -> Option<V3<Length<Meters>>> {
        self.dimensions
    }

    /// A set of arbitrary physical characteristics, if any.
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// How and where was the work inscribed with attribution
    pub fn inscription(&self) -> Option<&str> {
        self.inscription.as_deref()
    }

    /// Markings and other properties of a work that can be used to uniquely identify
    /// that work with confidence.
    pub fn markings(&self) -> Option<&str> {
        self.markings.as_deref()
    }

    /// Additional identifying marks that are less obvious to the naked eye.
    pub fn watermarks(&self) -> Option<&str> {
        self.watermarks.as_deref()
    }

    // -- Setters --

    /// Set the medium of the physical data. See getter for details.
    pub fn set_medium(&mut self, medium: impl ToString) {
        self.medium = Some(medium.to_string());
    }

    /// Set the dimensions text of the physical data. See getter for details.
    pub fn set_dimensions_display(&mut self, dimensions_display: impl ToString) {
        self.dimensions_display = Some(dimensions_display.to_string());
    }

    /// Set the dimensions of the physical data. See getter for details.
    pub fn set_dimensions(&mut self, dimensions: V3<Length<Meters>>) {
        self.dimensions = Some(dimensions);
    }

    /// Add a measurements to the physical data. See getter for details.
    pub fn add_measurement(&mut self, measurement: Measurement) {
        self.measurements.push(measurement);
    }

    /// Set the inscription of the physical data. See getter for details.
    pub fn set_inscription(&mut self, inscription: impl ToString) {
        self.inscription = Some(inscription.to_string());
    }

    /// Set the markings of the physical data. See getter for details.
    pub fn set_markings(&mut self, markings: impl ToString) {
        self.markings = Some(markings.to_string());
    }

    /// Set the watermarks of the physical data. See getter for details.
    pub fn set_watermarks(&mut self, watermarks: impl ToString) {
        self.watermarks = Some(watermarks.to_string());
    }

    // -- Builder Pattern --

    /// Set the medium of the physical data. See getter for details.
    pub fn with_medium(mut self, medium: impl ToString) -> Self {
        self.medium = Some(medium.to_string());
        self
    }

    /// Set the dimensions text of the physical data. See getter for details.
    pub fn with_dimensions_display(mut self, dimensions_display: impl ToString) -> Self {
        self.dimensions_display = Some(dimensions_display.to_string());
        self
    }

    /// Set the dimensions of the physical data. See getter for details.
    pub fn with_dimensions(mut self, dimensions: V3<Length<Meters>>) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    /// Add a measurements to the physical data. See getter for details.
    pub fn with_measurements(mut self, measurements: impl Iterator<Item = Measurement>) -> Self {
        self.measurements = measurements.collect();
        self
    }

    /// Set the inscription of the physical data. See getter for details.
    pub fn with_inscription(mut self, inscription: impl ToString) -> Self {
        self.inscription = Some(inscription.to_string());
        self
    }

    /// Set the markings of the physical data. See getter for details.
    pub fn with_markings(mut self, markings: impl ToString) -> Self {
        self.markings = Some(markings.to_string());
        self
    }

    /// Set the watermarks of the physical data. See getter for details.
    pub fn with_watermarks(mut self, watermarks: impl ToString) -> Self {
        self.watermarks = Some(watermarks.to_string());
        self
    }
}

/// Who created this work, where when, and in what circumstances. What has happened to it since.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct History {
    // Who created the work
    attribution: Option<String>,

    // Typically a "Last, First" version of attribution.
    attribution_sort_key: Option<String>,

    // A one-line description of the creation timeline.
    display_date: Option<String>,

    // Extracted begin/end creation years, if known or relevant.
    begin_year: Option<i64>,
    end_year: Option<i64>,

    // TODO: TAGME?: 20 broad time categories for when an artwork was created.
    time_category: String, // 20 broad categories

    // What do we know about where this object has been?
    provenance: Option<String>,

    // Text acknowledging the source or origin of the artwork and the year the object was acquired by the museum.
    credit_line: Option<String>,
}

impl History {
    // -- Getters --

    /// Attributing art is a complex subject. Ideally we'd just format the artist constituents
    /// and not have an extra string. Unfortunately, sometimes the artist has been lost to history.
    /// As with the artist name, we do still want to capture something of a work's origin and
    /// the research in its discovery in that case in a way that is easy to quickly review.
    /// Hence: the attribution string.
    pub fn attribution(&self) -> Option<&str> {
        self.attribution.as_deref()
    }

    /// Sorting of attributions is an even stickier topic than attributions itself. For single-
    /// artist works, in a western setting, it may be more suitable in some cases to sort by
    /// "last" name. Given how tricky names are in the context of all cultures throughout all
    /// history, we capture a second string for sort ordering attributions and punt the problem
    /// to the curators.
    pub fn attribution_sort_key(&self) -> Option<&str> {
        self.attribution_sort_key.as_deref()
    }

    /// A string representation of a human date. Similar to attribution, it would be ideal if we
    /// could just put a `jiff::Date` here and be done. Unfortunately, normal date libraries
    /// struggle to even describe dates with large negative year and uncertain date ranges.
    /// Again we put it on the curators to format a string that is useful.
    pub fn display_date(&self) -> Option<&str> {
        self.display_date.as_deref()
    }

    /// The integer begin/end year indicate either approximate or exact work create timespans.
    pub fn begin_year(&self) -> Option<i64> {
        self.begin_year
    }

    /// The integer begin/end year indicate either approximate or exact work create timespans.
    pub fn end_year(&self) -> Option<i64> {
        self.end_year
    }

    /// Physical history of this work.
    pub fn provenance(&self) -> Option<&str> {
        self.provenance.as_deref()
    }

    /// The entities that preserved this work until it was provided to this collection.
    pub fn credit_line(&self) -> Option<&str> {
        self.credit_line.as_deref()
    }

    // -- Setters --

    /// Set an attribution string on this entry. See getter for details.
    pub fn set_attribution(&mut self, attribution: impl ToString) {
        self.attribution = Some(attribution.to_string());
    }

    /// Set an attribution sort key string on this history. See getter for details.
    pub fn set_attribution_sort_key(&mut self, attribution_sort_key: impl ToString) {
        self.attribution_sort_key = Some(attribution_sort_key.to_string());
    }

    /// Set a display date string on this history. See getter for details.
    pub fn set_display_date(&mut self, display_date: impl ToString) {
        self.display_date = Some(display_date.to_string());
    }

    /// Set a begin_year on the history. See getter for details.
    pub fn set_begin_year(&mut self, begin_year: i64) {
        self.begin_year = Some(begin_year);
    }

    //; Set a end_year on the history. See getter for details.
    pub fn set_end_year(&mut self, end_year: i64) {
        self.end_year = Some(end_year);
    }

    /// Set a provenance string on this entry. See getter for details.
    pub fn set_provenance(&mut self, provenance: impl ToString) {
        self.provenance = Some(provenance.to_string());
    }

    /// Set a credit_line string on this entry. See getter for details.
    pub fn set_credit_line(&mut self, credit_line: impl ToString) {
        self.credit_line = Some(credit_line.to_string());
    }

    // -- Builder Pattern --

    /// Add an attribution string on this history. See getter for details.
    #[must_use]
    pub fn with_attribution(mut self, attribution: impl ToString) -> Self {
        self.attribution = Some(attribution.to_string());
        self
    }

    /// Add an attribution sort key string on this history. See getter for details.
    #[must_use]
    pub fn with_attribution_sort_key(mut self, attribution_sort_key: impl ToString) -> Self {
        self.attribution_sort_key = Some(attribution_sort_key.to_string());
        self
    }

    /// Add a display date string on this history. See getter for details.
    #[must_use]
    pub fn with_display_date(mut self, display_date: impl ToString) -> Self {
        self.display_date = Some(display_date.to_string());
        self
    }

    /// Add a begin_year on the history. See getter for details.
    #[must_use]
    pub fn with_begin_year(mut self, begin_year: i64) -> Self {
        self.begin_year = Some(begin_year);
        self
    }

    /// Add an end_year on the history. See getter for details.
    #[must_use]
    pub fn with_end_year(mut self, end_year: i64) -> Self {
        self.end_year = Some(end_year);
        self
    }

    /// Add a provenance string on this history. See getter for details.
    #[must_use]
    pub fn with_provenance(mut self, provenance: impl ToString) -> Self {
        self.provenance = Some(provenance.to_string());
        self
    }

    /// Add a credit_line string on this history. See getter for details.
    #[must_use]
    pub fn with_credit_line(mut self, credit_line: impl ToString) -> Self {
        self.credit_line = Some(credit_line.to_string());
        self
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
    // -- Getters --

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

    // -- Setters --

    /// Add a custodian to the location; e.g. a gallery or curator.
    pub fn set_custody(&mut self, custody: impl ToString) {
        self.custody = Some(custody.to_string());
    }

    /// Add site information to the location; e.g. which building of a gallery is the work in.
    pub fn set_site(&mut self, site: impl ToString) {
        self.site = Some(site.to_string());
    }

    /// Add room information to the location; e.g. which room of the gallery is the work in.
    pub fn set_room(&mut self, room: impl ToString) {
        self.room = Some(room.to_string());
    }

    /// Add position information to the location; e.g. where is the work in the room.
    pub fn set_position(&mut self, position: impl ToString) {
        self.position = Some(position.to_string());
    }

    /// Add a description to the location; e.g. how does the work appear in the display.
    pub fn set_description(&mut self, description: impl ToString) {
        self.description = Some(description.to_string());
    }

    /// Add a flag to the location to indicate if the object is currently on display.
    pub fn set_on_display(&mut self, on_display: bool) {
        self.on_display = Some(on_display);
    }

    // -- Builder Pattern --

    /// Add a custodian to the location; e.g. a gallery or curator.
    #[must_use]
    pub fn with_custody(mut self, custody: impl ToString) -> Self {
        self.custody = Some(custody.to_string());
        self
    }

    /// Add site information to the location; e.g. which building of a gallery is the work in.
    #[must_use]
    pub fn with_site(mut self, site: impl ToString) -> Self {
        self.site = Some(site.to_string());
        self
    }

    /// Add room information to the location; e.g. which room of the gallery is the work in.
    #[must_use]
    pub fn with_room(mut self, room: impl ToString) -> Self {
        self.room = Some(room.to_string());
        self
    }

    /// Add position information to the location; e.g. where is the work in the room.
    #[must_use]
    pub fn with_position(mut self, position: impl ToString) -> Self {
        self.position = Some(position.to_string());
        self
    }

    /// Add a description to the location; e.g. how does the work appear in the display.
    #[must_use]
    pub fn with_description(mut self, description: impl ToString) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Add a flag to the location to indicate if the object is currently on display.
    #[must_use]
    pub fn with_on_display(mut self, on_display: bool) -> Self {
        self.on_display = Some(on_display);
        self
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
