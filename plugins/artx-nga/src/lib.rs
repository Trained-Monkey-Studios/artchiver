use artchiver_sdk::*;
use extism_pdk::*;
use jiff::{Timestamp, civil::Date};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

import_section!();

#[expect(unused)]
#[derive(Clone, Debug, Deserialize)]
pub struct NgaObject {
    // [
    // "0", // objectid
    // "1", // accessioned
    // "1937.1.2.c", // accessionnum
    // "", // locationid **** listed as int
    // "Saint James Major", // title
    // "c. 1310", // displaydate
    // "1310", // beginyear
    // "1310", // endyear
    // "1300 to 1400", // visualbrowsertimespan
    // "tempera on panel", // medium
    // "painted surface (top of gilding): 62.2 × 34.8 cm (24 1/2 × 13 11/16 in.)\r\npainted surface (including painted border): 64.8 × 34.8 cm (25 1/2 × 13 11/16 in.)\r\noverall: 66.7 × 36.7 × 1.2 cm (26 1/4 × 14 7/16 × 1/2 in.)",
    // "", // inscription
    // "", // markings
    // "Grifo di Tancredi", // attributioninverted
    // "Grifo di Tancredi", // attribution
    // "By 1808 in the collection of ....",
    // "Andrew W. Mellon Collection", // creditline
    // "Painting", // classification
    // "", // subclassification
    // "painting", // visualbrowserclassification
    // "34", // parentid
    // "0", // isvirtual
    // "CIS-R", // departmentabbr
    // "", // portfolio
    // "", // series
    // "", // volume
    // "", // watermarks
    // "2023-05-09 17:01:03.48-04", // lastdetectedmodification
    // "Q20172973", // wikidataid
    // "" // customprinturl
    // ]
    objectid: i64,
    accessioned: i64,
    accessionnum: String,
    locationid: String,
    title: String,
    displaydate: String,
    beginyear: Option<i64>,
    endyear: Option<i64>,
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
    parentid: Option<i64>,
    isvirtual: i64,
    departmentabbr: String,
    portfolio: String,
    series: String,
    volume: String,
    watermarks: String,
    lastdetectedmodification: Timestamp,
    wikidataid: String,
    customprinturl: String,
}
const OBJECTS_URL: &str =
    "https://github.com/NationalGalleryOfArt/opendata/raw/refs/heads/main/data/objects.csv";

#[derive(Clone, Debug, Deserialize)]
pub struct NgaTerm {
    termid: i64,
    objectid: i64,
    termtype: String,
    term: String,
    #[allow(unused)]
    visualbrowsertheme: String,
    #[allow(unused)]
    visualbrowserstyle: String,
}
const TERMS_URL: &str = "https://raw.githubusercontent.com/NationalGalleryOfArt/opendata/refs/heads/main/data/objects_terms.csv";

#[expect(unused)]
#[derive(Clone, Debug, Deserialize)]
pub struct NgaPublishedImage {
    uuid: Uuid,
    iiifurl: String,
    iiifthumburl: String,
    viewtype: String,
    sequence: String,
    width: i32,
    height: i32,
    maxpixels: Option<i64>,
    created: Timestamp,
    modified: Timestamp,
    depictstmsobjectid: i64,
    assistivetext: String,
}
const PUBLISHED_IMAGES_URL: &str = "https://raw.githubusercontent.com/NationalGalleryOfArt/opendata/refs/heads/main/data/published_images.csv";

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
    Ok(csv_reader(csv)?
        .records()
        .flatten()
        .map(|row| match row.deserialize::<NgaTerm>(None) {
            Ok(r) => r,
            Err(e) => {
                Log::error(format!("Failed to deserialize term: {e}")).ok();
                Log::error(format!("Row is: {row:#?}")).ok();
                panic!("Failed to deserialize term: {e}")
            }
        })
        .collect())
}

fn objects(csv: &str) -> FnResult<HashMap<i64, NgaObject>> {
    Ok(csv_reader(csv)?
        .records()
        .flatten()
        .map(|row| {
            let r = match row.deserialize::<NgaObject>(None) {
                Ok(r) => r,
                Err(e) => {
                    Log::error(format!("Failed to deserialize object: {e}")).ok();
                    Log::error(format!("Row is: {row:#?}")).ok();
                    panic!("Failed to deserialize object: {e}")
                }
            };
            (r.objectid, r)
        })
        .collect())
}

fn published_images(csv: &str) -> FnResult<Vec<NgaPublishedImage>> {
    Ok(csv_reader(csv)?
        .records()
        .flatten()
        .map(|row| match row.deserialize::<NgaPublishedImage>(None) {
            Ok(r) => r,
            Err(e) => {
                Log::error(format!("Failed to deserialize published image: {e}")).ok();
                Log::error(format!("Row is: {row:#?}")).ok();
                panic!("Failed to deserialize published image: {e}")
            }
        })
        .collect())
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    Progress::percent(0, 4)?;

    let terms_cvs = Web::fetch_text(Request::get(TERMS_URL))?;
    let terms = terms(&terms_cvs)?;
    Progress::percent(1, 4)?;

    // Collect all unique terms into tags, stripping the objectid association.
    let mut all = HashSet::new();
    for term in &terms {
        let tag_kind = match term.termtype.as_str() {
            "Keyword" => TagKind::Default,
            "School" => TagKind::School,
            "Place Executed" => TagKind::Location,
            "Technique" => TagKind::Technique,
            "Systematic Catalogue Volume" => TagKind::Series,
            "Theme" => TagKind::Theme,
            "Style" => TagKind::Style,
            v => panic!("Unknown term type: {v}"),
        };
        let tag = Tag::new(term.term.to_owned())
            .with_remote_id(term.termid)
            .with_kind(tag_kind);
        all.insert(tag);
    }
    Progress::percent(2, 4)?;

    // Sort tags into an ordered vec for return.
    let mut all = all.drain().collect::<Vec<_>>();
    all.sort();
    Progress::percent(3, 4)?;

    // Count all terms with the same name (and implicitly different object association).
    // Note: tags has stripped the objectid, so counting terms per tag is the count of objects
    //       associated with that tag.
    for tag in &mut all {
        let count = terms.iter().filter(|t| t.term == tag.name()).count();
        tag.set_work_count(count.try_into()?);
    }

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
        if term.term == tag {
            obj_ids_with_tag.insert(term.objectid, Vec::new());
        }
    }
    Progress::percent(4, 6)?;

    // For each object with the tag, collect all tags on that object so we can construct the Work.
    for term in &terms {
        if obj_ids_with_tag.contains_key(&term.objectid) {
            obj_ids_with_tag
                .get_mut(&term.objectid)
                .unwrap()
                .push(term.term.to_owned());
        }
    }
    Progress::percent(5, 6)?;

    // Construct each work.
    let mut works = Vec::new();
    for (obj_id, tags) in &obj_ids_with_tag {
        let Some(img) = published_images
            .iter()
            .find(|i| i.depictstmsobjectid == *obj_id)
        else {
            Log::warn(format!("No published image found for object {obj_id}"))?;
            continue;
        };

        let obj = objects
            .get(obj_id)
            .expect("no object with id for matching term");
        works.push(
            Work::new(
                &obj.title,
                Date::new(obj.beginyear.unwrap_or(0).try_into()?, 1, 1)?,
                // Note: this appears to mostly just be a pre-baked call to the iiifurl.
                &img.iiifthumburl,
                // Note: max size that the server will send us back, not actual max size;
                //       native quality, not native image
                format!("{}/full/max/0/native.jpg", img.iiifurl),
                tags.clone(),
            )
            .with_remote_id(obj_id.to_string())
            // Note: archive url is for the iiif tile server and path
            .with_archive_url(img.iiifurl.to_owned()),
        );
    }

    Progress::clear()?;
    Ok(works.into())
}
