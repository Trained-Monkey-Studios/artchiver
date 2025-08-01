use artchiver_sdk::*;
use extism_pdk::*;
use jiff::civil::Date;
use serde::Deserialize;
use std::collections::HashMap;

import_section!();

/*
Column                      |           Type           | Collation | Nullable | Default
----------------------------+--------------------------+-----------+----------+---------
objectid                    | integer                  |           |          |
accessioned                 | integer                  |           |          |
accessionnum                | character varying(32)    |           |          |
locationid                  | integer                  |           |          |
title                       | character varying(2048)  |           |          |
displaydate                 | character varying(256)   |           |          |
beginyear                   | integer                  |           |          |
endyear                     | integer                  |           |          |
visualbrowsertimespan       | character varying(32)    |           |          |
medium                      | character varying(2048)  |           |          |
dimensions                  | character varying(2048)  |           |          |
inscription                 | character varying        |           |          |
markings                    | character varying        |           |          |
attributioninverted         | character varying(1024)  |           |          |
attribution                 | character varying(1024)  |           |          |
provenancetext              | character varying        |           |          |
creditline                  | character varying(2048)  |           |          |
classification              | character varying(64)    |           |          |
subclassification           | character varying(64)    |           |          |
visualbrowserclassification | character varying(32)    |           |          |
parentid                    | integer                  |           |          |
isvirtual                   | integer                  |           |          |
departmentabbr              | character varying(32)    |           |          |
portfolio                   | character varying(2048)  |           |          |
series                      | character varying(850)   |           |          |
volume                      | character varying(850)   |           |          |
watermarks                  | character varying(512)   |           |          |
lastdetectedmodification    | timestamp with time zone |           |          |
wikidataid                  | character varying(64)    |           |          |
customprinturl              | character varying(512)   |           |          |
 */
const OBJECTS_URL: &str =
    "https://github.com/NationalGalleryOfArt/opendata/raw/refs/heads/main/data/objects.csv";

#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        PluginMetadata::new(
            "The National Gallery of Art",
            "0.0.1",
            "A plugin for Artchiver to provide The National Gallery of the Art (nga.gov) open data.",
        )
        .with_rate_limit(1, 2.0),
    ))
}

fn make_reader(raw: &str) -> FnResult<csv::Reader<&[u8]>> {
    let rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    Ok(rdr)
}

fn get_record_tags(record: &csv::StringRecord) -> impl Iterator<Item = (&str, &str)> {
    record
        .get(51)
        .unwrap()
        .split('|')
        .filter(|s| !s.is_empty())
        .zip(record.get(53).unwrap().split('|').filter(|s| !s.is_empty()))
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    Progress::spinner()?;
    let raw = Web::fetch_text(Request::get(OBJECTS_URL))?;
    let mut rdr = make_reader(&raw)?;
    for result in rdr.records() {
        let record = result?;
        Log::info(format!("{record:?}"))?;
    }
    /*
    let mut all_names: HashMap<String, usize> = HashMap::new();
    let mut wiki_map: HashMap<String, String> = HashMap::new();
    let raw = Web::fetch_text(Request::get(CSV_URL))?;
    let mut rdr = make_reader(&raw)?;
    for result in rdr.records() {
        let record = result?;
        let pairs = get_record_tags(&record).map(|s| s.to_owned());
        for (tag, wiki) in pairs {
            all_names
                .entry(tag.to_owned())
                .and_modify(|c| *c += 1)
                .or_insert(1);
            wiki_map.insert(tag.to_owned(), wiki.to_owned());
        }
    }
    Log::info(format!("found {} tags", all_names.len()))?;
    info!("Found {} tags", all_names.len());
    Progress::clear()?;
    Ok(all_names
        .drain()
        .map(|(tag, count)| {
            Tag::new(
                &tag,
                TagKind::default(),
                Some(count as u64),
                wiki_map.get(&tag).cloned(),
            )
        })
        .collect::<Vec<Tag>>()
        .into())
     */
    Progress::clear()?;
    Ok(Vec::new().into())
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct SearchResults {
    total: u32,
    objectIDs: Vec<u32>,
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
struct ElementMeasurement {
    Width: Option<f32>,
    Height: Option<f32>,
    Depth: Option<f32>,
}

#[allow(non_snake_case, unused)]
#[derive(Debug, Deserialize)]
struct Measurement {
    elementName: String,
    elementDescription: Option<String>,
    elementMeasurements: ElementMeasurement,
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
    measurements: Option<Vec<Measurement>>,
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
    metadataDate: String,
    repository: String,
    objectURL: String,
    tags: Vec<Tag>,
    objectWikidata_URL: String,
    isTimelineWork: bool,
    GalleryNumber: String,
}

const URL: &str = "https://collectionapi.metmuseum.org";
const SEARCH_PATH: &str = "/public/collection/v1/search";
const OBJECTS_PATH: &str = "/public/collection/v1/objects";

#[plugin_fn]
pub fn list_works_for_tag(tag: String) -> FnResult<Json<Vec<Work>>> {
    /*
    // Query the search api with tags= to get the list of works by id
    let req = Request::get(URL)
        .in_path(SEARCH_PATH)
        .add_query("tags", "true")
        .add_query("q", &tag);
    let search_results = Web::fetch_text(req)?;
    let search = serde_json::from_str::<SearchResults>(&search_results)?;
    info!("Found {} works matching tag {tag}", search.total);

    // Query the Object API for each id we found above to get the data we need
    let mut matching_works = Vec::new();
    for (i, obj_id) in search.objectIDs.iter().enumerate() {
        Progress::percent(i.try_into()?, search.objectIDs.len().try_into()?)?;
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
        let object = serde_json::from_str::<ObjectInfo>(&object_info)?;
        if !object.primaryImage.is_empty() {
            matching_works.push(Work::new(
                object.title,
                0,
                Date::strptime("%Y-%m-%d", format!("{}-01-01", object.objectBeginDate))?,
                object.primaryImageSmall.to_owned(),
                object.primaryImage.to_owned(),
                None,
                object.tags.iter().map(|t| &t.term).cloned().collect(),
            ));
        }
    }
    Progress::clear()?;
    Ok(matching_works.into())
     */
    Ok(Vec::new().into())
}
