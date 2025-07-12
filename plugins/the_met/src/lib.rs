use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PluginMetadata {
    name: String,
    version: String,
    description: String,
}

#[host_fn]
extern "ExtismHost" {
    fn fetch_text(url: &str) -> String;
}

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
    Ok(Json(PluginMetadata {
        name: "The Metropolitan Gallery of Art".to_owned(),
        version: "0.0.1".to_owned(),
        description:
            "A plugin for Artchiver to provide The Metropolitan Gallery of the Arts open data."
                .to_owned(),
    }))
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<String>>> {
    let mut all_tags = HashSet::new();
    let raw = unsafe { fetch_text(CSV_URL)? };
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    for result in rdr.records() {
        let record = result?;
        let tags = record
            .get(51)
            .unwrap()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned());
        all_tags.extend(tags);
    }
    info!("Found {} tags", all_tags.len());
    Ok(all_tags.drain().collect::<Vec<String>>().into())
}
