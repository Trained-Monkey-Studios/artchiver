use artchiver_sdk::*;
use extism_pdk::*;
use jiff::civil::Date;
use serde::Deserialize;
use std::collections::HashMap;

import_section!();

// 0: Object Number,
// 1: Is Highlight,
// 2: Is Timeline Work,
// 3: Is Public Domain,
// 4: Object ID,
// 5: Gallery Number,
// 6: Department,
// 7: AccessionYear,
// 8: Object Name,
// 9: Title,
// 10: Culture,
// 11: Period,
// 12: Dynasty,
// 13: Reign,
// 14: Portfolio,
// 15: Constituent ID,
// 16: Artist Role,
// 17: Artist Prefix,
// 18: Artist Display Name,
// 19: Artist Display Bio,
// 20: Artist Suffix,
// 21: Artist Alpha Sort,
// 22: Artist Nationality,
// 23: Artist Begin Date,
// 24: Artist End Date,
// 25: Artist Gender,
// 26: Artist ULAN URL,
// 27: Artist Wikidata URL,
// 28: Object Date,
// 29: Object Begin Date,
// 30: Object End Date,
// 31: Medium,
// 32: Dimensions,
// 33: Credit Line,
// 34: Geography Type,
// 35: City,
// 36: State,
// 37: County,
// 38: Country,
// 39: Region,
// 40: Subregion,
// 41: Locale,
// 42: Locus,
// 43: Excavation,
// 44: River,
// 45: Classification,
// 46: Rights and Reproduction,
// 47: Link Resource,
// 48: Object Wikidata URL,
// 49: Metadata Date,
// 50: Repository,
// 51: Tags,
// 52: Tags AAT URL,
// 53: Tags Wikidata URL
const CSV_URL: &str = "https://media.githubusercontent.com/media/metmuseum/openaccess/refs/heads/master/MetObjects.csv";

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

fn make_reader(raw: &str) -> FnResult<csv::Reader<&[u8]>> {
    // let raw = unsafe { fetch_text(CSV_URL)? };
    let rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    Ok(rdr)
}

fn get_record_tags(record: &csv::StringRecord) -> impl Iterator<Item = &str> {
    record.get(51).unwrap().split('|').filter(|s| !s.is_empty())
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<TagInfo>>> {
    Progress::spinner()?;
    let mut all_names: HashMap<String, usize> = HashMap::new();
    let raw = Web::fetch_text(Request::get(CSV_URL))?;
    let mut rdr = make_reader(&raw)?;
    for result in rdr.records() {
        let record = result?;
        let tags = get_record_tags(&record).map(|s| s.to_owned());
        for tag in tags {
            all_names.entry(tag).and_modify(|c| *c += 1).or_insert(1);
        }
    }
    Log::info(format!("found {} tags", all_names.len()))?;
    info!("Found {} tags", all_names.len());
    Progress::clear()?;
    Ok(all_names
        .drain()
        .map(|(k, v)| TagInfo::new(k, TagKind::default(), Some(v as u64)))
        .collect::<Vec<TagInfo>>()
        .into())
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
struct Tag {
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
        let Ok(object_info) = Web::fetch_text(req) else {
            Log::error(format!("Failed to fetch object info for {obj_id}"))?;
            continue;
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
}
