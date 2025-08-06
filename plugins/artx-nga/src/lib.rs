use artchiver_sdk::*;
use extism_pdk::*;
use uuid::Uuid;
use jiff::{
    Zoned,
    civil::Date
};
use serde::Deserialize;
use std::collections::{
    HashSet,
    HashMap
};

import_section!();

#[derive(Clone, Debug, Deserialize)]
pub struct NgaObject {
    objectid: i64,
    accessioned: i64,
    accessionnum: String,
    locationid: i64,
    title: String,
    displaydate: String,
    beginyear: i64,
    endyear: i64,
    visualbrowsertimespan: String,
    medium: String,
    dimensions: String,
    inscription: String,
    markings: String,
    attributioninverted: String,
    attribution: String,
    provenancetext: String,
    creditline: String,
    classification: String,
    subclassification: String,
    visualbrowserclassification: String,
    parentid: i64,
    isvirtual: i64,
    departmentabbr: String,
    portfolio: String,
    series: String,
    volume: String,
    watermarks: String,
    lastdetectedmodification: Zoned,
    wikidataid: String,
    customprinturl: String,
}
const OBJECTS_URL: &str =
    "https://github.com/NationalGalleryOfArt/opendata/raw/refs/heads/main/data/objects.csv";

#[derive(Clone, Debug, Deserialize)]
#[allow(unused)]
pub struct NgaTerm {
    termid: i64,
    objectid: i64,
    termtype: String,
    term: String,
    visualbrowsertheme: String,
    visualbrowserstyle: String,
}
const TERMS_URL: &str =
    "https://raw.githubusercontent.com/NationalGalleryOfArt/opendata/refs/heads/main/data/objects_terms.csv";

#[derive(Clone, Debug, Deserialize)]
pub struct NgaPublishedImage {
    uuid: Uuid,
    iiifurl: String,
    iiifthumburl: String,
    viewtype: String,
    sequence: String,
    width: i32,
    height: i32,
    maxpixels: i64,
    created: Zoned,
    modified: Zoned,
    depictstmsobjectid: i64,
    assistivetext: String,
}
const PUBLISHED_IMAGES_URL: &str =
    "https://raw.githubusercontent.com/NationalGalleryOfArt/opendata/refs/heads/main/data/published_images.csv";

#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        PluginMetadata::new(
            "The National Gallery of Art",
            "0.0.1",
            "A plugin for Artchiver to provide The National Gallery of the Art (artx-nga.gov) open data.",
        )
        .with_rate_limit(1, 2.0),
    ))
}

fn csv_reader(raw: &str) -> FnResult<csv::Reader<&[u8]>> {
    let rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    Ok(rdr)
}

fn terms(csv: &str) -> FnResult<Vec<NgaTerm>> {
    Ok(csv_reader(&csv)?.records().flatten().map(|row| {
        let r: NgaTerm = row.deserialize(None).unwrap();
        r
    }).collect())
}

fn objects(csv: &str) -> FnResult<HashMap<i64, NgaObject>> {
    Ok(csv_reader(&csv)?.records().flatten().map(|row| {
        let r: NgaObject = row.deserialize(None).unwrap();
        (r.objectid, r)
    }).collect())
}

fn published_images(csv: &str) -> FnResult<Vec<NgaPublishedImage>> {
    Ok(csv_reader(&csv)?.records().flatten().map(|row| {
        let r: NgaPublishedImage = row.deserialize(None).unwrap();
        r
    }).collect())
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    Progress::spinner()?;

    let terms_cvs = Web::fetch_text(Request::get(TERMS_URL))?;
    let terms = terms(&terms_cvs)?;

    let mut all = HashSet::new();
    for term in &terms {
        if term.termtype == "Keyword" {
            all.insert(Tag::new(term.term.to_owned(), None, None::<String>));
        }
    }
    let mut all = all.drain().collect::<Vec<_>>();
    all.sort();

    Progress::clear()?;
    Ok(all.into())
}

#[plugin_fn]
pub fn list_works_for_tag(tag: String) -> FnResult<Json<Vec<Work>>> {
    Progress::percent(0, 6)?;

    let terms_cvs = Web::fetch_text(Request::get(TERMS_URL))?;
    let terms = terms(&terms_cvs)?;
    Progress::percent(1, 6)?;

    let objects_csv = Web::fetch_text(Request::get(OBJECTS_URL))?;
    let objects = objects(&objects_csv)?;
    Progress::percent(2, 6)?;

    let published_images_csv = Web::fetch_text(Request::get(PUBLISHED_IMAGES_URL))?;
    let published_images = published_images(&published_images_csv)?;
    Progress::percent(3, 6)?;

    // Find all objects that have the given tag.
    let mut obj_ids_with_tag: HashMap<i64, Vec<String>> = HashMap::new();
    for term in &terms {
        if term.termtype == "Keyword" && term.term == tag {
            obj_ids_with_tag.insert(term.objectid, Vec::new());
        }
    }
    Progress::percent(4, 6)?;

    // For each object with the tag, collect all tags on that object so we can construct the Work.
    for term in &terms {
        if term.termtype == "Keyword" && obj_ids_with_tag.contains_key(&term.objectid) {
            obj_ids_with_tag.get_mut(&term.objectid).unwrap().push(term.term.to_owned());
        }
    }
    Progress::percent(5, 6)?;

    // Construct each work.
    let mut works = Vec::new();
    for (obj_id, tags) in &obj_ids_with_tag {
        let Some(img) = published_images.iter().find(|i| i.depictstmsobjectid == *obj_id) else {
            Log::warn(format!("No published image found for object {}", obj_id))?;
            continue;
        };

        let obj = objects.get(obj_id).expect("no object with id for matching term");
        works.push(Work::new(
            &obj.title,
            Date::new(obj.beginyear.try_into()?, 1, 1)?,
            // Note: full image, rescaled to 512x512
            format!("{}/full/512,512/0/default.jpg", img.iiifurl),
            // Note: max size that the server will send us back, not actual max size;
            //       native quality, not native image
            format!("{}/full/max/0/native.jpg", img.iiifurl),
            tags.clone(),
        )
            .with_remote_id(obj_id.to_string())
            // Note: archive url is for the iiif tile server and path
            .with_archive_url(img.iiifurl.to_owned()));
    }

    Progress::clear()?;
    Ok(works.into())
}
