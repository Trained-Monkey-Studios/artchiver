use artchiver_sdk::*;
use extism_pdk::*;
use rss::Channel;
use std::{collections::HashMap, time::Duration};

import_section!();

#[plugin_fn]
pub fn startup() -> FnResult<Json<PluginMetadata>> {
    Ok(Json(
        PluginMetadata::new(
            "Podcatcher Plugin for Artchiver",
            "0.0.1",
            "A plugin for Artchiver to download podcasts via RSS feeds.",
        )
        .with_cache_timeout(Duration::from_secs(60 * 60))
        .with_rate_limit(1, 1.0)
        .with_configuration("Podcasts", ConfigKind::StringList),
    ))
}

pub fn tags_for_item(channel: &Channel, item: &rss::Item) -> Vec<Tag> {
    let mut tags: Vec<Tag> = item
        .categories
        .iter()
        .map(|c| Tag::new(c.name.clone()))
        .collect();
    if let Some(ext) = item.itunes_ext.as_ref() {
        tags.extend(
            ext.keywords
                .as_deref()
                .unwrap_or_default()
                .split(',')
                .map(Tag::new),
        );
    }
    tags.push(
        Tag::new(channel.title.clone())
            .with_kind(TagKind::Series)
            .with_wiki_url(channel.link.clone()),
    );
    tags
}

#[plugin_fn]
pub fn list_tags() -> FnResult<Json<Vec<Tag>>> {
    let config = Config::get_string_list("Podcasts")?;
    let mut acc = HashMap::new();
    for url in config {
        let content = Web::fetch_text(Request::get(&url))?;
        let channel = Channel::read_from(content.as_bytes())?;

        //Log::debug(format!("Channel: {channel:#?}"))?;

        // All works in the channel should get bundled with a tag with the title of the channel
        Log::trace(format!(
            "Found {} works in channel: {}",
            channel.items.len(),
            channel.title
        ))?;
        for item in &channel.items {
            let item_tags = tags_for_item(&channel, item);
            for tag in item_tags {
                *acc.entry(tag).or_default() += 1;
            }
        }
        Log::trace(format!("Accumulated {} total tags", acc.len()))?;
    }

    let mut out = Vec::new();
    for (tag, count) in acc.into_iter() {
        out.push(tag.with_remote_work_count(count));
    }
    Ok(out.into())
}

#[plugin_fn]
pub fn list_works_for_tag(tag: String) -> FnResult<Json<Vec<Work>>> {
    Progress::percent(0, 6)?;
    let config = Config::get_string_list("Podcasts")?;
    let mut works = Vec::new();
    for url in config {
        let content = Web::fetch_text(Request::get(&url))?;
        let channel = Channel::read_from(content.as_bytes())?;

        for (i, item) in channel.items.iter().enumerate() {
            let item_tags = tags_for_item(&channel, item);
            if tag == channel.title || item.categories.iter().any(|c| c.name == tag) {
                let Some(enclosure) = item.enclosure.as_ref() else {
                    Log::trace(format!("Skipping item {i} due to missing enclosure"))?;
                    continue;
                };
                let pub_date = item
                    .pub_date
                    .as_ref()
                    .and_then(|s| jiff::fmt::rfc2822::parse(s).ok().map(|z| z.date()));
                Log::trace(format!("Parsed Date: {pub_date:?}"))?;
                let image = if let Some(ext) = item.itunes_ext.as_ref()
                    && let Some(image) = &ext.image
                {
                    image.clone()
                } else if let Some(ext) = &channel.itunes_ext.as_ref()
                    && let Some(image) = &ext.image
                {
                    image.clone()
                } else {
                    panic!("Missing image");
                };

                works.push(Work::new(
                    item.title
                        .clone()
                        .unwrap_or_else(|| format!("{} - {i}", channel.title)),
                    pub_date.expect("Invalid or missing pubDate"),
                    image,
                    enclosure.url.clone(),
                    item_tags.iter().map(|t| t.name().to_owned()).collect(),
                ));
            }
        }
    }
    Ok(works.into())
}
