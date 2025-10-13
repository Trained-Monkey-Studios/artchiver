use artchiver_sdk::*;
use extism_pdk::*;
use jiff::{Timestamp, civil::Date};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

import_section!();

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

#[expect(unused)]
#[derive(Clone, Debug, Deserialize)]
pub struct NgaObject {
    /*
    // Core Info
    // ---------
    title: String,
    // Empty or a wiki reference number to look up, like r"Q\d+"
    // Maybe map to a URL
    wikidataid: String,
    // Mostly empty, except for 500 they're selling prints for
    // Would be cool to expose these to boost the gallery; could we add a tag for 'Prints Available'
    // or somesuch and have a buy-the-artwork link?
    customprinturl: String,

    // Internal (custom props?)
    // --------
    // Unique integer id for each work.
    objectid: i64,
    // Is just a 1, always.
    accessioned: i64,
    // The unique id of the work.
    accessionnum: String,
    // Inside baseball for the collection. Not useful to us, except maybe as a tag.
    departmentabbr: String,
    // Unclear if this is the preservation history or a timestamp for this record itself.
    // 29% of records were last modified on May 9, 2023, so probably an update to these records.
    // Will treat as metadata for now.
    lastdetectedmodification: Timestamp,

    // Hierarchy
    // ---------
    parentid: Option<i64>,
    // TODO: some "works" appear to be recursive folder nodes that collect works; I think isvirtual identifies these nodes;
    //       see if we can use these virtual nodes to add additional tags to "real" works; at the very least, filter them out.
    isvirtual: i64,

    // History / Creation Info
    // --------
    attribution: String,         // creator
    attributioninverted: String, // last, first; if relevant
    displaydate: String,         // description of creation timeline
    beginyear: Option<i64>,
    endyear: Option<i64>,
    visualbrowsertimespan: String, // TAGME: 20 broad categories
    provenancetext: String,
    creditline: String,

    // PhysicalInfo
    // ------------
    medium: String,
    dimensions: String,
    inscription: String,
    // Identifying marks
    markings: String,
    // Also identifying marks, but less obvious
    watermarks: String,
    // custody and location
    locationid: String, // -> expand this out; help people see it in the real world, if desired

    // Not exactly physical, but important metadata about where the work came from
    // ╭────┬──────────────────────────┬───────┬──────────┬────────────┬───────────────────────────────────────────────╮
    // │  # │      classification      │ count │ quantile │ percentage │                   frequency                   │
    // ├────┼──────────────────────────┼───────┼──────────┼────────────┼───────────────────────────────────────────────┤
    // │  0 │ Print                    │ 64765 │     0.45 │ 45.07%     │ ********************************************* │
    // │  1 │ Photograph               │ 21258 │     0.15 │ 14.79%     │ **************                                │
    // │  2 │ Drawing                  │ 18299 │     0.13 │ 12.73%     │ ************                                  │
    // │  3 │ Index of American Design │ 18259 │     0.13 │ 12.71%     │ ************                                  │
    // │  4 │ Portfolio                │  7605 │     0.05 │ 5.29%      │ *****                                         │
    // │  5 │ Sculpture                │  4676 │     0.03 │ 3.25%      │ ***                                           │
    // │  6 │ Painting                 │  4401 │     0.03 │ 3.06%      │ ***                                           │
    // │  7 │ Volume                   │  3276 │     0.02 │ 2.28%      │ **                                            │
    // │  8 │ Decorative Art           │   747 │     0.01 │ 0.52%      │                                               │
    // │  9 │ Technical Material       │   365 │     0.00 │ 0.25%      │                                               │
    // │ 10 │ Time-Based Media Art     │    56 │     0.00 │ 0.04%      │                                               │
    // ╰────┴──────────────────────────┴───────┴──────────┴────────────┴───────────────────────────────────────────────╯
    classification: String,
    // ╭───┬─────────────────────────────┬───────┬──────────┬────────────┬───────────────────────────────────────────────╮
    // │ # │ visualbrowserclassification │ count │ quantile │ percentage │                   frequency                   │
    // ├───┼─────────────────────────────┼───────┼──────────┼────────────┼───────────────────────────────────────────────┤
    // │ 0 │ print                       │ 64765 │     0.45 │ 45.07%     │ ********************************************* │
    // │ 1 │ drawing                     │ 36558 │     0.25 │ 25.44%     │ *************************                     │
    // │ 2 │ photograph                  │ 21258 │     0.15 │ 14.79%     │ **************                                │
    // │ 3 │ portfolio                   │  7605 │     0.05 │ 5.29%      │ *****                                         │
    // │ 4 │ sculpture                   │  4676 │     0.03 │ 3.25%      │ ***                                           │
    // │ 5 │ painting                    │  4401 │     0.03 │ 3.06%      │ ***                                           │
    // │ 6 │ volume                      │  3276 │     0.02 │ 2.28%      │ **                                            │
    // │ 7 │ decorative art              │   747 │     0.01 │ 0.52%      │                                               │
    // │ 8 │ technical material          │   365 │     0.00 │ 0.25%      │                                               │
    // │ 9 │ new media                   │    56 │     0.00 │ 0.04%      │                                               │
    // ╰───┴─────────────────────────────┴───────┴──────────┴────────────┴───────────────────────────────────────────────╯
    visualbrowserclassification: String,
    // ╭────┬───────────────────────┬────────┬──────────┬────────────┬────────────────────────────────────────────────────────────────────────╮
    // │  # │   subclassification   │ count  │ quantile │ percentage │                               frequency                                │
    // ├────┼───────────────────────┼────────┼──────────┼────────────┼────────────────────────────────────────────────────────────────────────┤
    // │  0 │                       │ 101071 │     0.70 │ 70.33%     │ ********************************************************************** │
    // │  1 │ Drawing               │  18557 │     0.13 │ 12.91%     │ ************                                                           │
    // │  2 │ Print                 │  10713 │     0.07 │ 7.45%      │ *******                                                                │
    // │  3 │ Contact Sheet         │   2967 │     0.02 │ 2.06%      │ **                                                                     │
    // │  4 │ Historical Portrait   │   2388 │     0.02 │ 1.66%      │ *                                                                      │
    // │  5 │ Medal/Medallion       │   2022 │     0.01 │ 1.41%      │ *                                                                      │
    // │  6 │ Work Print            │   1336 │     0.01 │ 0.93%      │                                                                        │
    // │  7 │ Archival              │   1245 │     0.01 │ 0.87%      │                                                                        │
    // │  8 │ Plaquette             │    651 │     0.00 │ 0.45%      │                                                                        │
    // │  9 │ Ceramic               │    450 │     0.00 │ 0.31%      │                                                                        │
    // │ 10 │ Small Bronze          │    331 │     0.00 │ 0.23%      │                                                                        │
    // │ 11 │ Bust                  │    187 │     0.00 │ 0.13%      │                                                                        │
    // │ 12 │ Photograph            │    187 │     0.00 │ 0.13%      │                                                                        │
    // │ 13 │ Multiple              │    139 │     0.00 │ 0.10%      │                                                                        │
    // │ 14 │ Playing Card          │    126 │     0.00 │ 0.09%      │                                                                        │
    // │ 15 │ Statuette             │    126 │     0.00 │ 0.09%      │                                                                        │
    // │ 16 │ Printmaking Matrices  │    112 │     0.00 │ 0.08%      │                                                                        │
    // │ 17 │ Relief                │    108 │     0.00 │ 0.08%      │                                                                        │
    // │ 18 │ Miniature             │    107 │     0.00 │ 0.07%      │                                                                        │
    // │ 19 │ Furniture             │    103 │     0.00 │ 0.07%      │                                                                        │
    // │ 20 │ Textile               │    101 │     0.00 │ 0.07%      │                                                                        │
    // │ 21 │ Statue                │     99 │     0.00 │ 0.07%      │                                                                        │
    // │ 22 │ Utilitarian Object    │     99 │     0.00 │ 0.07%      │                                                                        │
    // │ 23 │ Niello Plate          │     78 │     0.00 │ 0.05%      │                                                                        │
    // │ 24 │ Coin                  │     60 │     0.00 │ 0.04%      │                                                                        │
    // │ 25 │ Endpaper              │     49 │     0.00 │ 0.03%      │                                                                        │
    // │ 26 │ Molded Paper          │     36 │     0.00 │ 0.03%      │                                                                        │
    // │ 27 │ Applique              │     33 │     0.00 │ 0.02%      │                                                                        │
    // │ 28 │ Drawing/Print         │     28 │     0.00 │ 0.02%      │                                                                        │
    // │ 29 │ Collage               │     25 │     0.00 │ 0.02%      │                                                                        │
    // │ 30 │ Antiquities           │     21 │     0.00 │ 0.01%      │                                                                        │
    // │ 31 │ Enamel                │     21 │     0.00 │ 0.01%      │                                                                        │
    // │ 32 │ Head                  │     18 │     0.00 │ 0.01%      │                                                                        │
    // │ 33 │ Figure Fragment       │     17 │     0.00 │ 0.01%      │                                                                        │
    // │ 34 │ Liturgical Object     │     13 │     0.00 │ 0.01%      │                                                                        │
    // │ 35 │ Negative              │     12 │     0.00 │ 0.01%      │                                                                        │
    // │ 36 │ Jewelry               │     10 │     0.00 │ 0.01%      │                                                                        │
    // │ 37 │ Mobile                │     10 │     0.00 │ 0.01%      │                                                                        │
    // │ 38 │ Rock Crystal          │     10 │     0.00 │ 0.01%      │                                                                        │
    // │ 39 │ Stabile               │     10 │     0.00 │ 0.01%      │                                                                        │
    // │ 40 │ Maquette              │      7 │     0.00 │ 0.00%      │                                                                        │
    // │ 41 │ Medallic Die          │      6 │     0.00 │ 0.00%      │                                                                        │
    // │ 42 │ Architectural Element │      4 │     0.00 │ 0.00%      │                                                                        │
    // │ 43 │ Death Mask            │      4 │     0.00 │ 0.00%      │                                                                        │
    // │ 44 │ Proof Sheet           │      3 │     0.00 │ 0.00%      │                                                                        │
    // │ 45 │ Stained Glass         │      3 │     0.00 │ 0.00%      │                                                                        │
    // │ 46 │ Armor                 │      2 │     0.00 │ 0.00%      │                                                                        │
    // │ 47 │ Mask                  │      1 │     0.00 │ 0.00%      │                                                                        │
    // │ 48 │ Reliquary Object      │      1 │     0.00 │ 0.00%      │                                                                        │
    // ├────┼───────────────────────┼────────┼──────────┼────────────┼────────────────────────────────────────────────────────────────────────┤
    // │  # │   subclassification   │ count  │ quantile │ percentage │                               frequency                                │
    // ╰────┴───────────────────────┴────────┴──────────┴────────────┴────────────────────────────────────────────────────────────────────────╯
    subclassification: String,
    // Was the work part of a larger collection of the artists works?
    portfolio: String,
    // What is its position within a broader series of works?
    series: String,
    // Was the work part of a larger volume of works?
    volume: String,
    */
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

#[derive(Clone, Debug, Deserialize)]
pub struct NgaLocation {
    locationid: i64,
    site: String,
    room: String,
    publicaccess: i64,
    description: String,
    unitposition: String,
}
const LOCATIONS_URL: &str = "https://raw.githubusercontent.com/NationalGalleryOfArt/opendata/refs/heads/main/data/locations.csv";

fn locations(csv: &str) -> FnResult<Vec<NgaLocation>> {
    Ok(csv_reader(csv)?
        .records()
        .flatten()
        .map(|row| match row.deserialize::<NgaLocation>(None) {
            Ok(r) => r,
            Err(e) => {
                Log::error(format!("Failed to deserialize location: {e}")).ok();
                Log::error(format!("Row is: {row:#?}")).ok();
                panic!("Failed to deserialize location: {e}")
            }
        })
        .collect())
}

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

const DISPLAY_TAG: &str = "On Display";

fn type_to_kind(termtype: &str) -> TagKind {
    match termtype {
        "Keyword" => TagKind::Default,
        "School" => TagKind::School,
        "Place Executed" => TagKind::Location,
        "Technique" => TagKind::Technique,
        "Systematic Catalogue Volume" => TagKind::Series,
        "Theme" => TagKind::Theme,
        "Style" => TagKind::Style,
        v => panic!("Unknown term type: {v}"),
    }
}

fn location_tag_names(loc: &NgaLocation) -> [String; 2] {
    [
        format!("NGA {}", loc.site),
        format!("NGA {}", loc.description),
    ]
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    Progress::percent(0, 100)?;

    Log::info("Downloading locations list...")?;
    let locations_csv = Web::fetch_text(Request::get(LOCATIONS_URL))?;
    let locations = locations(&locations_csv)?;
    Progress::percent(3, 100)?;

    Log::info("Downloading terms list...")?;
    let terms_csv = Web::fetch_text(Request::get(TERMS_URL))?;
    let terms = terms(&terms_csv)?;
    Progress::percent(10, 100)?;

    // Log::info("Downloading objects list...")?;
    // let objects_csv = Web::fetch_text(Request::get(OBJECTS_URL))?;
    // let objects = objects(&objects_csv)?;
    // Progress::percent(10, 100)?;

    // Terms is like a join model in that there are multiple rows linking a term to an object.
    // The term data is inline in the table and repeated however. The upshot is that we just need
    // to traverse terms once and count the number of instances with the same name.
    let delta = 90. / terms.len() as f64;
    let mut position = 10.;
    let mut term_tags = HashMap::<&str, Tag>::new();
    for term in &terms {
        Progress::percent(position as i32, 100)?;
        position += delta;
        term_tags
            .entry(&term.term)
            .or_insert_with(|| {
                Tag::new(term.term.to_owned())
                    .with_remote_id(term.termid)
                    .with_kind(type_to_kind(&term.termtype))
            })
            .increment_work_count();
    }

    // We also want to tag works that are in the same room or building.
    let mut loc_tags = HashMap::<String, Tag>::new();
    for loc in &locations {
        for name in location_tag_names(loc) {
            loc_tags
                .entry(name.clone())
                .or_insert_with(|| Tag::new(name).with_kind(TagKind::Location))
                .increment_work_count();
        }
    }

    // We also want a tag for everything on display.
    let display_tag = Tag::new(DISPLAY_TAG).with_remote_work_count(locations.len().try_into()?);

    // Sort tags into an ordered vec for return.
    let mut all = vec![display_tag];
    all.extend(term_tags.into_values());
    all.extend(loc_tags.into_values());
    all.sort();

    Progress::clear()?;
    Ok(all.into())
}

#[plugin_fn]
pub fn list_works_for_tag(tag_name: String) -> FnResult<Json<Vec<Work>>> {
    Progress::percent(0, 100)?;

    Log::info("Downloading locations list...")?;
    let locations_csv = Web::fetch_text(Request::get(LOCATIONS_URL))?;
    let locations = locations(&locations_csv)?;
    Progress::percent(1, 100)?;

    Log::info("Downloading terms list...")?;
    let terms_csv = Web::fetch_text(Request::get(TERMS_URL))?;
    let terms = terms(&terms_csv)?;
    Progress::percent(2, 100)?;

    Log::info("Downloading published images list...")?;
    let published_images_csv = Web::fetch_text(Request::get(PUBLISHED_IMAGES_URL))?;
    let published_images = published_images(&published_images_csv)?;
    Progress::percent(5, 100)?;

    Log::info("Downloading objects list (this may take awhile)...")?;
    let objects_csv = Web::fetch_text(Request::get(OBJECTS_URL))?;
    let objects = objects(&objects_csv)?;
    Progress::percent(10, 100)?;

    // Map from the tag name to all of the tags matching that name. We need to check if the tag
    // is a term, or a location, or "On Display". Each of these checks requires a different
    // strategy for performance.

    // Since terms is like a join table, we can scan it for the associated objects.
    let mut obj_ids = terms
        .iter()
        .filter(|t| t.term == tag_name)
        .map(|t| t.objectid)
        .collect::<Vec<i64>>();
    // This may be a location tag instead of a term tag, so check locations next.
    if obj_ids.is_empty() {
        // There is no backref from the location rows to the object, so we have to iterate the
        // objects, finding matching tags. We need to allocate the tag names to check if they match
        // the requested tag, so visit the locations first, since that list is much smaller.
        let loc_ids = locations
            .iter()
            .filter(|l| location_tag_names(l).contains(&tag_name))
            .map(|l| l.locationid)
            .collect::<HashSet<i64>>();
        obj_ids = objects
            .values()
            .filter(|o| !o.locationid.is_empty()) // remove items without a location
            .flat_map(|o| {
                o.locationid
                    .parse::<i64>()
                    .map(|loc_id| (o.objectid, loc_id))
            }) // parse to an integer, keeping objectid around
            .filter(|(_, loc_id)| loc_ids.contains(loc_id)) // remove objects without matching locations rows
            .map(|(obj_id, _)| obj_id) // Get just the object id
            .collect();
    }
    // This may be the on-display tag instead of a term or location tag.
    if tag_name == DISPLAY_TAG {
        assert!(
            obj_ids.is_empty(),
            "did not find a matching term or location"
        );
        obj_ids = objects
            .values()
            .filter(|o| !o.locationid.is_empty())
            .map(|o| o.objectid)
            .collect();
    }
    Log::info(format!(
        "Found {} objects with tag '{tag_name}'",
        obj_ids.len()
    ))?;
    Progress::percent(15, 100)?;
    let mut pos = 15.;
    let fract = (100. - pos) / obj_ids.len() as f64;

    // Iterate the objects and build a work for each. The hard part here is the reverse tag lookup.
    // We need to get all the tags that apply to an object, not just the tag we're looking up.
    let mut works = Vec::new();
    for obj_id in &obj_ids {
        Progress::percent(pos as i32, 100)?;
        pos += fract;
        let obj = objects.get(obj_id).expect("linked object");

        // Search for all terms we're listed in and grab tag names.
        let mut obj_tags = terms
            .iter()
            .filter(|t| t.objectid == *obj_id)
            .map(|t| t.term.clone())
            .collect::<Vec<String>>();
        // If the location field is non-null, then it _will_ be On Display and have room and site tags.
        if !obj.locationid.is_empty()
            && let Ok(loc_id) = obj.locationid.parse::<i64>()
        {
            let loc = locations
                .iter()
                .find(|l| l.locationid == loc_id)
                .expect("no matching location");
            obj_tags.push(DISPLAY_TAG.to_owned());
            for name in location_tag_names(loc) {
                obj_tags.push(name);
            }
        }

        // Find any image.
        let Some(img) = published_images
            .iter()
            .find(|i| i.depictstmsobjectid == *obj_id)
        else {
            Log::warn(format!("No published image found for object {obj_id}"))?;
            continue;
        };

        // Create a location record.
        let mut loc = Location::default().with_custody("National Gallery of Art");
        if let Ok(location_id) = obj.locationid.parse::<i64>()
            && let Some(location) = locations.iter().find(|l| l.locationid == location_id)
        {
            loc = loc
                .with_site(&location.site)
                .with_room(&location.room)
                .with_on_display(location.publicaccess == 1)
                .with_description(&location.description)
                .with_position(&location.unitposition);
        }

        // Put together the work
        let work = Work::new(
            &obj.title,
            Date::new(obj.beginyear.unwrap_or(0).try_into()?, 1, 1)?,
            // Note: this appears to mostly just be a pre-baked call to the iiifurl.
            &img.iiifthumburl,
            // Note: max size that the server will send us back, not actual max size;
            //       native quality, not native image
            format!("{}/full/max/0/native.jpg", img.iiifurl),
            obj_tags.iter().map(|s| s.to_string()).collect(),
        )
        .with_remote_id(obj_id.to_string())
        // Note: archive url is for the iiif tile server and path
        .with_archive_url(img.iiifurl.to_owned())
        .with_location(loc);
        works.push(work);
    }

    Progress::clear()?;
    Ok(works.into())
}
