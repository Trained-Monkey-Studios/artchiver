use crate::model::MetadataPool;
use bevy::prelude::*;
use std::ops::Range;

#[derive(Debug)]
struct TagCountCache {
    db_gen: u64,
    filter: String,
    cache: i64,
}

#[derive(Debug)]
struct TagCache {
    db_gen: u64,
    filter: String,
    range: Range<usize>,
    cache: Vec<String>,
}

#[derive(Debug)]
pub struct CachingPool {
    pool: MetadataPool,
    database_generation: u64,
    tag_count_cache: Option<TagCountCache>,
    tag_cache: Option<TagCache>,
}

impl CachingPool {
    pub(crate) fn new(pool: MetadataPool) -> Self {
        Self {
            pool,
            database_generation: 0,
            tag_count_cache: None,
            tag_cache: None,
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
            return Ok(cache.cache);
        }
        let value = self.pool.tags_count(filter)?;
        self.tag_count_cache = Some(TagCountCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            cache: value,
        });
        Ok(value)
    }

    pub fn tags_list(&mut self, range: Range<usize>, filter: &str) -> Result<Vec<String>> {
        if let Some(cache) = self.tag_cache.as_ref()
            && cache.db_gen == self.database_generation
            && cache.filter == filter
            && cache.range == range
        {
            return Ok(cache.cache.clone());
        }
        let value = self.pool.tags_list(range.clone(), filter)?;
        self.tag_cache = Some(TagCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            range,
            cache: value,
        });
        Ok(self.tag_cache.as_ref().unwrap().cache.clone())
    }
}
