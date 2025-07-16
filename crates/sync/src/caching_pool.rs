use crate::{model::MetadataPool, shared::TagSet};
use artchiver_sdk::{TagInfo, Work};
use bevy::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

#[derive(Debug)]
struct TagCountCache {
    db_gen: u64,
    filter: String,
    count: i64,
}

#[derive(Debug)]
struct TagCache {
    db_gen: u64,
    filter: String,
    range: Range<usize>,
    tags: Vec<TagInfo>,
}

#[derive(Debug)]
struct TagPluginCache {
    db_gen: u64,
    plugins: HashSet<String>,
}

#[derive(Debug)]
pub struct CachingPool {
    pool: MetadataPool,
    database_generation: u64,
    tag_count_cache: Option<TagCountCache>,
    tag_cache: Option<TagCache>,
    tag_plugins_cache: HashMap<String, TagPluginCache>,
}

impl CachingPool {
    pub(crate) fn new(pool: MetadataPool) -> Self {
        Self {
            pool,
            database_generation: 0,
            tag_count_cache: None,
            tag_cache: None,
            tag_plugins_cache: HashMap::new(),
        }
    }

    pub(crate) fn bump_generation(&mut self) {
        self.database_generation += 1;
    }

    pub fn tags_count(&mut self, filter: &str) -> Result<i64> {
        if let Some(cache) = &self.tag_count_cache
            && cache.db_gen == self.database_generation
            && cache.filter == filter
        {
            return Ok(cache.count);
        }
        let count = self.pool.tags_count(filter)?;
        self.tag_count_cache = Some(TagCountCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            count,
        });
        Ok(count)
    }

    pub fn tags_list(&mut self, range: Range<usize>, filter: &str) -> Result<Vec<TagInfo>> {
        if let Some(cache) = self.tag_cache.as_ref()
            && cache.db_gen == self.database_generation
            && cache.filter == filter
            && cache.range == range
        {
            return Ok(cache.tags.clone());
        }
        let tags = self.pool.tags_list(range.clone(), filter)?;
        self.tag_cache = Some(TagCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            range,
            tags,
        });
        Ok(self.tag_cache.as_ref().unwrap().tags.clone())
    }

    pub fn list_plugins_for_tag(&mut self, tag: &str) -> Result<HashSet<String>> {
        if let Some(cache) = self.tag_plugins_cache.get(tag)
            && cache.db_gen == self.database_generation
        {
            return Ok(cache.plugins.clone());
        }
        let plugins = self.pool.list_plugins_for_tag(tag)?;
        self.tag_plugins_cache.insert(
            tag.to_owned(),
            TagPluginCache {
                db_gen: self.database_generation,
                plugins: plugins.clone(),
            },
        );
        Ok(plugins)
    }

    pub fn works_list(&mut self, tags: &TagSet) -> Result<Vec<Work>> {
        self.pool.works_list(0..100, tags)
    }
}
