use artchiver_sdk::*;
use extism_pdk::*;
use jiff::civil::Date;
use std::time::Duration;

// Most of the code required to interface with Artchiver is imported above via artchiver_sdk.
// Extism does require a macro around our imports, so this macro saves us copying that boilerplate.
import_section!();

/// The startup method gets called by the Artchiver when it detects this wasm file.
/// Since configuration via config:: must be supplied at build time, if the user changes
/// the configuration values, the plugin may get restarted between calls to startup and
/// other APIs. For this reason, don't store pointers between startup and other calls.
#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        // The plugin metadata block tells Artchiver how to show the plugin in the UX.
        PluginMetadata::new(
            // Pick a name that is short and to the point.
            "Artchiver Demo Plugin",
            // Plugins use semantic versioning.
            "0.0.1",
            // Also include a longer description of what the plugin is for.
            "A plugin to demonstrate Artchiver's API and explain how to build a plugin to integrate a new data source.",
        )
        // Configure the plugin's HTTP rate limiter to allow a maximum of 10 requests in a
        // 1 second interval. Please follow all rate guidance of any provider queried.
        .with_rate_limit(10, 1.0)
        // By default, Artchiver will only re-download a link requested by a plugin after a week.
        // Instead, set a 1 day default cache timeout for this example plugin.
        .with_cache_timeout(Duration::from_secs(24 * 60 * 60))
        // Plugins can accept configuration parameters from the UX, such as login and password.
        // All parameters are provided to the plugin via `config::get`. See the Extism docs for
        // more information, or consult the example usage below.
        .with_configuration("Debug", ConfigKind::String),
    ))
}

// Tags are how we browse and find artwork in Artchiver. The `list_tags` method will be called
// when the user clicks on the Refresh Tags button next to the plugin's name in the UX.
// Plugins should find and return all tags that could be applied to works from our provider.
#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    // For this demo, we're going to use the words in the English dictionary.
    const URL: &str = "https://raw.githubusercontent.com/karthikramx/snippable-dictionary/refs/heads/main/english_dictionary.csv";

    // The Progress API lets us display our current state in the UX. We should make use of
    // it to provide feedback to the user during long-running operations.
    Progress::spinner()?;

    // We can also use the Log API to send messages to the messages tab under the plugin in the UX.
    Log::info(format!("Reading tags from {URL}"))?;

    // See the artchiver_sdk docs for more details on the Request object.
    let raw = Web::fetch_text(Request::get(URL))?;

    // Artchiver plugins can make use of any WASM-compatible crate in the Rust ecosystem.
    // Here we're making use of the fantastic `csv` crate to parse our data.
    let mut out = Vec::new();
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(raw.as_bytes());
    for result in rdr.records() {
        let record = result?;
        let word = record.get(1).unwrap();

        // The Tag object
        let tag = Tag::new(
            // Any valid Rust string (e.g. UTF-8) is a valid tag.
            word,
        )
        // We will generate a few works for each tag. Normally the plugin should look up
        // or count this number from the API, if possible. This number shows up next to
        // the tag in the UX so that the user can tell if they've got everything.
        .with_remote_work_count(3)
        // The wiki is a link to information about the tag, if it exists. Here we will
        // link to the dictionary.com page for the word, as an example.
        .with_wiki_url(format!("https://www.dictionary.com/browse/{word}"));

        // Accumulate the tag into our list to return to Artchiver.
        out.push(tag);
    }

    // Since plugins are WASM, we can only transfer basic types in and out. Fortunately,
    // Extism makes this very easy via its Json wrapper type, hence the `.into()`.
    Ok(out.into())
}

// When the user selects a tag to populate, this method gets called to find works matching
// that tag. This method returns a Work structure that contains various URLs that indicate
// the data. Artchiver takes care of safely and quickly fetching all the data URLs, storing
// the data into the data folder, and doing that in the background while allowing the user
// to continue browsing in the meantime.
#[plugin_fn]
pub fn list_works_for_tag(tag: String) -> FnResult<Json<Vec<Work>>> {
    // When a configuration is set in the UX, it will be available to the plugin via `config::get`.
    if config::get("Debug")?.as_deref() == Some("panic") {
        // This message will show up prominently at the top of the UX.
        panic!("Here is where you can find the message when a plugin panics.")
    }

    // For our purposes, we need to generate a handful of works that we can hand out to any tag.
    // A real plugin would reach out to an API with an open access policy.
    Ok(vec![
        Work::new(
            "Demo Work 01",
            Date::new(2022, 1, 1)?,
            "https://static.wikia.nocookie.net/nyancat/images/a/a1/Nyan_Cat_Power.png/revision/latest/scale-to-width-down/220",
            "https://static.wikia.nocookie.net/nyancat/images/a/a1/Nyan_Cat_Power.png/revision/latest/scale-to-width-down/1024",
            vec![tag.to_owned(), "Nyan_Cat".into()],
        ),
        Work::new(
            "Demo Work 02",
            Date::new(2022, 1, 2)?,
            "https://static.wikia.nocookie.net/nyancat/images/3/36/Nyan_Cat_Unlock_Power.gif/revision/latest/scale-to-width-down/220",
            "https://static.wikia.nocookie.net/nyancat/images/3/36/Nyan_Cat_Unlock_Power.gif/revision/latest/scale-to-width-down/1024",
            vec![tag.to_owned(), "Nyan_Cat".into()],
        ),
        Work::new(
            "Demo Work 03",
            Date::new(2022, 1, 3)?,
            "https://static.wikia.nocookie.net/nyancat/images/c/cd/Nyan_Cat_Ability.gif/revision/latest/scale-to-width-down/220",
            "https://static.wikia.nocookie.net/nyancat/images/c/cd/Nyan_Cat_Ability.gif/revision/latest/scale-to-width-down/1024",
            vec![tag.to_owned(), "Nyan_Cat".into()],
        ),
    ].into())
}
