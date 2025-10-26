use artchiver_sdk::*;
use extism_pdk::*;
use jiff::civil::Date;
use serde::Deserialize;
use std::collections::HashMap;

import_section!();
#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        PluginMetadata::new(
            "The Metropolitan Gallery of Art",
            "0.0.1",
            "A plugin for Artchiver to provide The Metropolitan Gallery of the Arts open data.",
        )
        .with_rate_limit(1, 2.0),
    ))
}

const DISPLAY_TAG: &str = "On Display";
const HIGHLIGHT_TAG: &str = "Met Highlight";
const TIMELINE_TAG: &str = "Met Timeline";

#[allow(unused)]
#[derive(Clone, Debug, Deserialize)]
struct CsvMetObject {
    object_number: String, // 0: Object Number, // Not unique. Seems to sometimes be a date, sometimes other things.
    is_highlight: String, // 1: Is Highlight, // values "True" or "False"; 0.57% of records; use for HIGHLIGHT_TAG
    is_timeline_work: String, // 2: Is Timeline Work, // values "True" or "False"; 1.65% of records; use for TIMELINE_TAG
    is_public_domain: String, // 3: Is Public Domain, // values "True" or "False"; 51.2% of records (200k+ per)
    object_id: i64,           // 4: Object ID, // Unique ID; appears to always be integer
    gallery_number: String, // 5: Gallery Number, // 89.78% empty (archived), but _lots_ of rooms at the met, not all are id's some are string place names
    // Worth using as a tag in full.
    // ╭────┬───────────────────────────────────────────┬────────┬──────────┬────────────┬─────────────────────────────────────╮
    // │  # │                Department                 │ count  │ quantile │ percentage │              frequency              │
    // ├────┼───────────────────────────────────────────┼────────┼──────────┼────────────┼─────────────────────────────────────┤
    // │  0 │ Drawings and Prints                       │ 172630 │     0.36 │ 35.60%     │ *********************************** │
    // │  1 │ European Sculpture and Decorative Arts    │  43051 │     0.09 │ 8.88%      │ ********                            │
    // │  2 │ Photographs                               │  37459 │     0.08 │ 7.72%      │ *******                             │
    // │  3 │ Asian Art                                 │  37000 │     0.08 │ 7.63%      │ *******                             │
    // │  4 │ Greek and Roman Art                       │  33726 │     0.07 │ 6.95%      │ ******                              │
    // │  5 │ Costume Institute                         │  31652 │     0.07 │ 6.53%      │ ******                              │
    // │  6 │ Egyptian Art                              │  27969 │     0.06 │ 5.77%      │ *****                               │
    // │  7 │ The American Wing                         │  18532 │     0.04 │ 3.82%      │ ***                                 │
    // │  8 │ Islamic Art                               │  15573 │     0.03 │ 3.21%      │ ***                                 │
    // │  9 │ Modern and Contemporary Art               │  14696 │     0.03 │ 3.03%      │ ***                                 │
    // │ 10 │ Arms and Armor                            │  13623 │     0.03 │ 2.81%      │ **                                  │
    // │ 11 │ Arts of Africa, Oceania, and the Americas │  12367 │     0.03 │ 2.55%      │ **                                  │
    // │ 12 │ Medieval Art                              │   7142 │     0.01 │ 1.47%      │ *                                   │
    // │ 13 │ Ancient Near Eastern Art                  │   6223 │     0.01 │ 1.28%      │ *                                   │
    // │ 14 │ Musical Instruments                       │   5227 │     0.01 │ 1.08%      │ *                                   │
    // │ 15 │ European Paintings                        │   2626 │     0.01 │ 0.54%      │                                     │
    // │ 16 │ Robert Lehman Collection                  │   2586 │     0.01 │ 0.53%      │                                     │
    // │ 17 │ The Cloisters                             │   2340 │     0.00 │ 0.48%      │                                     │
    // │ 18 │ The Libraries                             │    534 │     0.00 │ 0.11%      │                                     │
    // ╰────┴───────────────────────────────────────────┴────────┴──────────┴────────────┴─────────────────────────────────────╯
    department: String,     // 6: Department,
    accession_year: String, // 7: AccessionYear, // TAGME: 0.8% blank; mostly the year, going back to 1800's; several specific dates
    // Not the name; more like general category of medium. 20% are "Print", 6% are "Photograph".
    // Long tail with 28k+ entries, but less than the 65k+ medium entries.
    object_name: String, // 8: Object Name,
    title: String, // 9: Title, // Not unique! 6% are blank. Lots of random descriptions, but closer to a title.
    // 57% blank and 1 "unknown", but accurate below that, with a tail of 7300+ entries.
    // TAGME: Tag ones with 50 or more items.
    culture: String, // 10: Culture,
    // 81% blank, but useful below that. 1891 unique entries.
    // Tag ones with 50 or more items.
    period: String, // 11: Period,
    // 95% blank, unclear how useful these would be as tags.
    dynasty: String, // 12: Dynasty,
    // 98% blank, egyptian beyond; maybe useful as a tag.
    reign: String,                   // 13: Reign,
    portfolio: String, // 14: Portfolio, // 95% blank; very long instances, not worth tagging
    constituent_id: String, // 15: Constituent ID, // 42% blank; contains doubles?
    artist_role: String, // 16: Artist Role, // 42% blank; 24% "Artist"; seems like a job description of the artist; add to attribution section
    artist_prefix: String, // 17: Artist Prefix, // 42% blank; 30% differently blank; lots of bars as decoration; seems to have desriptives like "probably" and "published by"
    artist_display_name: String, // 18: Artist Display Name, // 41% blank; 1.5% Walker Evans; seems to be full attribution, with artists separated by | if multiple
    artist_display_bio: String, // 19: Artist Display Bio, // Mostly 1-liners with place and year range, commas separating multiple artists
    artist_suffix: String, // 20: Artist Suffix, // Seems to be mostly ", Place" or a bunch of |'s
    artist_alpha_sort: String, // 21: Artist Alpha Sort, // Mostly display name, but Last, First format
    artist_nationality: String, // 22: Artist Nationality,
    artist_begin_date: String, // 23: Artist Begin Date,
    artist_end_date: String,   // 24: Artist End Date,
    artist_gender: String,     // 25: Artist Gender,
    artist_ulan: String,       // 26: Artist ULAN URL,
    artist_wikidata_url: String, // 27: Artist Wikidata URL,
    object_date: String, // 28: Object Date, // very broad; sometimes a year int, sometimes a textual guess like "late 19th century"
    object_begin_date: String, // 29: Object Begin Date,
    object_end_date: String, // 30: Object End Date,
    medium: String,      // 31: Medium,
    dimensions: String,  // 32: Dimensions,
    credit_line: String, // 33: Credit Line,
    geography_type: String, // 34: Geography Type,
    city: String,        // 35: City,
    state: String,       // 36: State,
    county: String,      // 37: County,
    country: String,     // 38: Country,
    region: String,      // 39: Region,
    subregion: String,   // 40: Subregion,
    locale: String,      // 41: Locale,
    locus: String,       // 42: Locus,
    excavation: String,  // 43: Excavation,
    river: String,       // 44: River,
    classification: String, // 45: Classification,
    rights_and_reproduction: String, // 46: Rights and Reproduction,
    link_resource: String, // 47: Link Resource,
    object_wikidate_url: String, // 48: Object Wikidata URL,
    metadata_date: String, // 49: Metadata Date,
    repository: String,  // 50: Repository,
    tags: String,        // 51: Tags,
    tags_aat_url: String, // 52: Tags AAT URL,
    tags_wikidata_url: String, // 53: Tags Wikidata URL
}
const CSV_URL: &str = "https://media.githubusercontent.com/media/metmuseum/openaccess/refs/heads/master/MetObjects.csv";

fn csv_reader(raw: &str) -> FnResult<csv::Reader<&[u8]>> {
    let rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    Ok(rdr)
}

fn objects(csv: &str) -> FnResult<HashMap<i64, CsvMetObject>> {
    Ok(csv_reader(csv)?
        .records()
        .flatten()
        .map(|row| {
            let r = match row.deserialize::<CsvMetObject>(None) {
                Ok(r) => r,
                Err(e) => {
                    Log::error(format!("Failed to deserialize object: {e}")).ok();
                    Log::error(format!("Row is: {row:#?}")).ok();
                    panic!("Failed to deserialize object: {e}")
                }
            };
            (r.object_id, r)
        })
        .collect())
}

fn get_record_tags(obj: &CsvMetObject) -> impl Iterator<Item = (&str, &str)> {
    let wiki_urls = obj.tags_wikidata_url.split('|').filter(|s| !s.is_empty());
    let tags = obj.tags.split('|').filter(|s| !s.is_empty());
    tags.zip(wiki_urls)
}

const URL: &str = "https://collectionapi.metmuseum.org";
const OBJECTS_PATH: &str = "/public/collection/v1/objects";

fn room_tag_for_gallery_number(gallery_number: &str) -> Option<String> {
    if !gallery_number.is_empty() {
        if let Ok(num) = gallery_number.parse::<i16>() {
            Some(format!("The Met Room {num}"))
        } else {
            Some(format!("The Met {}", gallery_number.trim()))
        }
    } else {
        None
    }
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    Progress::percent(0, 100)?;

    Log::info("Downloading Met objects list (this may take awhile)...")?;
    let objects_csv = Web::fetch_text(Request::get(CSV_URL))?;
    Progress::percent(50, 100)?;

    Log::info("Importing Met objects...")?;
    let objects = objects(&objects_csv)?;
    Progress::percent(90, 100)?;

    let mut pos = 90.;
    let f = 10. / objects.len() as f64;

    let mut display_tag = Tag::new(DISPLAY_TAG);
    let mut highlight_tag = Tag::new(HIGHLIGHT_TAG);
    let mut timeline_tag = Tag::new(TIMELINE_TAG);

    Log::info("Scanning Met objects for tags...")?;
    let mut room_tags = HashMap::<String, Tag>::new();
    let mut term_tags = HashMap::<String, Tag>::new();
    for obj in objects.values() {
        Progress::percent(pos as i32, 100)?;
        pos += f;

        if obj.is_highlight == "True" {
            highlight_tag.increment_work_count();
        }
        if obj.is_timeline_work == "True" {
            timeline_tag.increment_work_count();
        }
        if let Some(room_tag) = room_tag_for_gallery_number(&obj.gallery_number) {
            display_tag.increment_work_count();
            room_tags
                .entry(room_tag.clone())
                .and_modify(|t| t.increment_work_count())
                .or_insert_with(|| Tag::new(room_tag).with_remote_work_count(1));
        }
        for (tag, wiki) in get_record_tags(obj) {
            term_tags
                .entry(tag.to_owned())
                .and_modify(|t| t.increment_work_count())
                .or_insert_with(|| Tag::new(tag).with_remote_work_count(1).with_wiki_url(wiki));
        }
    }
    Log::info(format!("found {} tag terms", term_tags.len()))?;
    Log::info(format!("found {} room terms", room_tags.len()))?;

    Log::info("Sorting tags...")?;
    let mut all_tags = term_tags.into_values().collect::<Vec<Tag>>();
    all_tags.extend(room_tags.into_values());
    all_tags.push(display_tag);
    all_tags.push(highlight_tag);
    all_tags.push(timeline_tag);
    all_tags.sort();
    Log::info(format!("Providing {} total tags", all_tags.len()))?;

    Progress::clear()?;
    Ok(all_tags.into())
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct Constituent {
    constituentID: u32,
    role: String,
    name: String,
    constituentULAN_URL: String,
    constituentWikidata_URL: String,
    gender: String,
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct MetElementMeasurement {
    Width: Option<f32>,
    Height: Option<f32>,
    Depth: Option<f32>,
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct MetMeasurement {
    elementName: String,
    elementDescription: Option<String>,
    elementMeasurements: MetElementMeasurement,
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct MetTag {
    term: String,
    AAT_URL: Option<String>,
    Wikidata_URL: Option<String>,
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct ObjectInfo {
    objectID: u32,
    isHighlight: bool,
    accessionNumber: String,
    accessionYear: String,
    isPublicDomain: bool,
    primaryImage: String,
    primaryImageSmall: String,
    additionalImages: Vec<String>,
    constituents: Option<Vec<Constituent>>,
    department: String,
    objectName: String,
    title: String,
    culture: String,
    period: String,
    dynasty: String,
    reign: String,
    portfolio: String,
    artistRole: String,
    artistPrefix: String,
    artistDisplayName: String,
    artistDisplayBio: String,
    artistSuffix: String,
    artistAlphaSort: String,
    artistNationality: String,
    artistBeginDate: String,
    artistEndDate: String,
    artistGender: String,
    artistWikidata_URL: String,
    artistULAN_URL: String,
    objectDate: String,
    objectBeginDate: i32,
    objectEndDate: i32,
    medium: String,
    dimensions: String,
    measurements: Option<Vec<MetMeasurement>>,
    creditLine: String,
    geographyType: String,
    city: String,
    state: String,
    county: String,
    country: String,
    region: String,
    subregion: String,
    locale: String,
    locus: String,
    excavation: String,
    river: String,
    classification: String,
    rightsAndReproduction: String,
    linkResource: String,
    #[allow(unused)] // always blank
    metadataDate: String,
    repository: String,
    objectURL: String,
    tags: Option<Vec<MetTag>>,
    objectWikidata_URL: String,
    isTimelineWork: bool,
    GalleryNumber: String,
}

#[plugin_fn]
pub fn list_works_for_tag(tag_name: String) -> FnResult<Json<Vec<Work>>> {
    Progress::percent(0, 100)?;

    let is_display_tag = tag_name == DISPLAY_TAG;
    let is_highlight_tag = tag_name == HIGHLIGHT_TAG;
    let is_timeline_tag = tag_name == TIMELINE_TAG;

    // Iterate the object csv to find any matching tags.
    Log::info("Downloading Met objects list (this may take awhile)...")?;
    let objects_csv = Web::fetch_text(Request::get(CSV_URL))?;
    Progress::percent(7, 100)?;

    Log::info("Importing Met objects...")?;
    let objects = objects(&objects_csv)?;
    Progress::percent(10, 100)?;

    Log::info("Searching objects for matching works...")?;
    let mut obj_ids = Vec::new();
    for (obj_id, obj) in &objects {
        let room_tag = room_tag_for_gallery_number(&obj.gallery_number);
        if is_display_tag && room_tag.is_some()
            || is_highlight_tag && obj.is_highlight == "True"
            || is_timeline_tag && obj.is_timeline_work == "True"
            || Some(&tag_name) == room_tag.as_ref()
            || get_record_tags(obj).any(|(t, _)| t == tag_name)
        {
            obj_ids.push(*obj_id);
        }
    }
    Log::info(format!(
        "Found {} works matching tag {tag_name}",
        obj_ids.len()
    ))?;
    Progress::percent(15, 100)?;

    let mut pos = 15.;
    let f = (100. - pos) / obj_ids.len() as f64;

    // Build Works for each found object.
    let mut all_works = Vec::new();
    for obj_id in &obj_ids {
        Progress::percent(pos as i32, 100)?;
        pos += f;

        // Query the Object API for each id we found above to get the data we need.
        // Note that the CSV is missing the image URLs, presumably so that we have to
        // call this API.
        let req = Request::get(URL)
            .in_path(OBJECTS_PATH)
            .append_path_segment(obj_id.to_string());
        let object_info = match Web::fetch_text(req) {
            Ok(s) => s,
            Err(TextFetchError::HttpError(code)) => {
                Log::error(format!(
                    "Failed to fetch object info for {obj_id}: HTTP {code}"
                ))?;
                continue;
            }
            Err(other) => {
                return Err(other.into());
            }
        };

        // Parse JSON into an ObjectInfo.
        let api_object = match serde_json::from_str::<ObjectInfo>(&object_info) {
            Ok(obj) => obj,
            Err(err) => {
                Log::error(format!("Failed to parse JSON object, {err}"))?;
                Log::error(format!("Document is: {object_info}"))?;
                return Err(err.into());
            }
        };
        if api_object.primaryImage.is_empty() {
            Log::warn(format!(
                "Missing image for object {}: {}",
                api_object.objectID, api_object.title
            ))?;
            continue;
        }

        // Collect tags
        let mut tags: Vec<String> = api_object
            .tags
            .unwrap_or_default()
            .iter()
            .map(|t| &t.term)
            .cloned()
            .collect();
        if api_object.isHighlight {
            tags.push(HIGHLIGHT_TAG.to_owned());
        }
        if api_object.isTimelineWork {
            tags.push(TIMELINE_TAG.to_owned());
        }

        // We don't have much information for location.
        let mut loc = Location::default().with_custody("The Metropolitan Gallery of Art");
        if let Some(room_tag) = room_tag_for_gallery_number(&api_object.GalleryNumber) {
            loc.set_room(api_object.GalleryNumber);
            tags.push(DISPLAY_TAG.to_owned());
            tags.push(room_tag);
        }

        // We have more information about the work history.
        let mut history = History::default()
            .with_begin_year(api_object.objectBeginDate.into())
            .with_end_year(api_object.objectEndDate.into());
        if !api_object.artistDisplayName.is_empty() {
            history.set_attribution(api_object.artistDisplayName);
        }
        if !api_object.artistAlphaSort.is_empty() {
            history.set_attribution_sort_key(api_object.artistAlphaSort);
        }
        if !api_object.objectDate.is_empty() {
            history.set_display_date(api_object.objectDate);
        }
        if !api_object.rightsAndReproduction.is_empty() {
            history.set_provenance(api_object.rightsAndReproduction);
        }
        if !api_object.creditLine.is_empty() {
            history.set_credit_line(api_object.creditLine);
        }

        let mut physical = PhysicalData::default();
        if !api_object.medium.is_empty() {
            physical.set_medium(&api_object.medium);
        }
        if !api_object.dimensions.is_empty() {
            physical.set_dimensions_display(&api_object.dimensions);
        }
        if let Some(measurements) = api_object.measurements.as_deref() {
            for measure in measurements {
                if let Some(width) = measure.elementMeasurements.Width {
                    physical.add_measurement(
                        Measurement::new(
                            width / 100., // documented as centimeters
                            SiUnit::Meter,
                        )?
                        .with_name(format!("{}-width", measure.elementName))
                        .with_description(
                            measure.elementDescription.as_deref().unwrap_or_default(),
                        ),
                    );
                }
                if let Some(height) = measure.elementMeasurements.Height {
                    physical.add_measurement(
                        Measurement::new(
                            height / 100., // documented as centimeters
                            SiUnit::Meter,
                        )?
                        .with_name(format!("{}-height", measure.elementName))
                        .with_description(
                            measure.elementDescription.as_deref().unwrap_or_default(),
                        ),
                    );
                }
                if let Some(depth) = measure.elementMeasurements.Depth {
                    physical.add_measurement(
                        Measurement::new(
                            depth / 100., // documented as centimeters
                            SiUnit::Meter,
                        )?
                        .with_name(format!("{}-depth", measure.elementName))
                        .with_description(
                            measure.elementDescription.as_deref().unwrap_or_default(),
                        ),
                    );
                }
            }
        }

        let work = Work::new(
            api_object.title,
            Date::strptime("%Y-%m-%d", format!("{}-01-01", api_object.objectBeginDate))
                .unwrap_or_default(),
            api_object.primaryImageSmall.replace(' ', "%20").to_owned(),
            api_object.primaryImage.replace(' ', "%20").to_owned(),
            tags,
        )
        .with_remote_id(obj_id)
        .with_location(loc)
        .with_history(history)
        .with_physical_data(physical);
        all_works.push(work);
    }
    Progress::clear()?;
    Ok(all_works.into())
}
