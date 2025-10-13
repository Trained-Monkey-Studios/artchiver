use artchiver_sdk::*;
use extism_pdk::*;
use jiff::civil::Date;
use serde::Deserialize;
use std::collections::HashMap;

import_section!();

// 0: Object Number, // Not unique. Seems to sometimes be a date, sometimes other things.
// 1: Is Highlight, // TAGME: 0.57% of records
// 2: Is Timeline Work, // TAGME: 1.65% of records
// 3: Is Public Domain, // 51.2% of records
// 4: Object ID, // Unique ID!
// 5: Gallery Number, // 89.78% empty (archived), but _lots_ of rooms at the met, not all are id's some are string place names
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
// 6: Department, // TAGME?
// 7: AccessionYear, // TAGME: 0.8% blank; mostly the year, going back to 1800's; several specific dates
// 8: Object Name, // Not the name. 20% are "Print", 6% are "Photograph". Seems like a broad description more than anything, even when specific.
// 9: Title, // Not unique! 6% are blank. Lots of random descriptions, but closer to a title.
// 10: Culture, // TAGME: 57% blank, but accurate below that, with a very long tail
// 11: Period, // TAGME: 81% blank, but useful below that
// 12: Dynasty, // TAGME: 95% blank, unclear how useful these would be as tags
// 13: Reign, // TAGME: 98% blank, egyptian beyond; maybe useful?
// 14: Portfolio, // 95% blank; very long instances, not worth tagging
// 15: Constituent ID, // 42% blank; contains doubles?
// 16: Artist Role, // 42% blank; 24% "Artist"; seems like a job description of the artist; add to attribution section
// 17: Artist Prefix, // 42% blank; 30% differently blank; lots of bars as decoration; seems to have desriptives like "probably" and "published by"
// 18: Artist Display Name, // 41% blank; 1.5% Walker Evans; seems to be full attribution, with artists separated by | if multiple
// 19: Artist Display Bio, // Mostly 1-liners with place and year range, commas separating multiple artists
// 20: Artist Suffix, // Seems to be mostly ", Place" or a bunch of |'s
// 21: Artist Alpha Sort, // Mostly display name, but Last, First format
// 22: Artist Nationality,
// 23: Artist Begin Date,
// 24: Artist End Date,
// 25: Artist Gender,
// 26: Artist ULAN URL,
// 27: Artist Wikidata URL,
// 28: Object Date, // very broad; sometimes a year int, sometimes a textual guess like "late 19th century"
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
            Tag::new(&tag)
                .with_remote_work_count(count as u64)
                .with_wiki_url(wiki_map.get(&tag).cloned().unwrap_or_default())
        })
        .collect::<Vec<Tag>>()
        .into())
}

#[allow(non_snake_case)]
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
    tags: Vec<MetTag>,
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
                Date::strptime("%Y-%m-%d", format!("{}-01-01", object.objectBeginDate))?,
                object.primaryImageSmall.replace(' ', "%20").to_owned(),
                object.primaryImage.replace(' ', "%20").to_owned(),
                object.tags.iter().map(|t| &t.term).cloned().collect(),
            ));
        }
    }
    Progress::clear()?;
    Ok(matching_works.into())
}
