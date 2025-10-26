use artchiver_sdk::*;
use extism_pdk::*;

import_section!();

#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        PluginMetadata::new(
            "The Smithsonian",
            "0.0.1",
            "A plugin for Artchiver to provide The Smithsonian's open data.",
        )
        .with_rate_limit(10, 1.0)
        .with_configuration("API Key", ConfigKind::String),
    ))
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    todo!("list tags");
}
