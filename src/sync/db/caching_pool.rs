use crate::{
    shared::tag::TagSet,
    sync::db::{
        model::MetadataPool,
        tag::{TagEntry, count_tags, list_plugins_for_tag, list_tags},
    },
};
use anyhow::Result;
use artchiver_sdk::Work;
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
struct TagsListCache {
    db_gen: u64,
    filter: String,
    range: Range<usize>,
    tags: Vec<TagEntry>,
}

#[derive(Debug)]
struct TagPluginCache {
    db_gen: u64,
    plugins: HashSet<String>,
}

#[derive(Debug)]
struct WorksCountCache {
    db_gen: u64,
    tag_set: TagSet,
    count: i64,
}

#[derive(Debug)]
struct WorksListCache {
    db_gen: u64,
    range: Range<usize>,
    tag_set: TagSet,
    works: Vec<Work>,
}

#[derive(Debug)]
struct WorksLookupCache {
    db_gen: u64,
    work_id: i64,
    work: Work,
}

#[derive(Debug)]
pub struct CachingPool {
    pool: MetadataPool,
    database_generation: u64,
    tag_count_cache: Option<TagCountCache>,
    tags_list_cache: Option<TagsListCache>,
    tag_plugins_cache: HashMap<String, TagPluginCache>,
    works_count_cache: Option<WorksCountCache>,
    works_list_cache: Option<WorksListCache>,
    works_lookup_cache: Option<WorksLookupCache>,
}

impl CachingPool {
    pub(crate) fn new(pool: MetadataPool) -> Self {
        Self {
            pool,
            database_generation: 0,
            tag_count_cache: None,
            tags_list_cache: None,
            tag_plugins_cache: HashMap::new(),
            works_count_cache: None,
            works_list_cache: None,
            works_lookup_cache: None,
        }
    }

    pub(crate) fn bump_generation(&mut self) {
        self.database_generation += 1;
    }

    pub fn count_tags(&mut self, filter: &str) -> Result<i64> {
        if let Some(cache) = &self.tag_count_cache
            && cache.db_gen == self.database_generation
            && cache.filter == filter
        {
            return Ok(cache.count);
        }
        let count = count_tags(&self.pool.get()?, filter)?;
        self.tag_count_cache = Some(TagCountCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            count,
        });
        Ok(count)
    }

    pub fn list_tags(&mut self, range: Range<usize>, filter: &str) -> Result<Vec<TagEntry>> {
        if let Some(cache) = self.tags_list_cache.as_ref()
            && cache.db_gen == self.database_generation
            && cache.filter == filter
            && cache.range == range
        {
            return Ok(cache.tags.clone());
        }
        let tags = list_tags(&self.pool.get()?, range.clone(), filter)?;
        let out = tags.clone();
        self.tags_list_cache = Some(TagsListCache {
            db_gen: self.database_generation,
            filter: filter.to_owned(),
            range,
            tags,
        });
        Ok(out)
    }

    pub fn list_plugins_for_tag(&mut self, tag: &str) -> Result<HashSet<String>> {
        if let Some(cache) = self.tag_plugins_cache.get(tag)
            && cache.db_gen == self.database_generation
        {
            return Ok(cache.plugins.clone());
        }
        let plugins = list_plugins_for_tag(&self.pool.get()?, tag)?;
        self.tag_plugins_cache.insert(
            tag.to_owned(),
            TagPluginCache {
                db_gen: self.database_generation,
                plugins: plugins.clone(),
            },
        );
        Ok(plugins)
    }

    pub fn works_count(&mut self, tag_set: &TagSet) -> Result<i64> {
        if let Some(cache) = self.works_count_cache.as_ref()
            && cache.db_gen == self.database_generation
            && &cache.tag_set == tag_set
        {
            return Ok(cache.count);
        }
        let count = self.pool.works_count(tag_set)?;
        self.works_count_cache = Some(WorksCountCache {
            db_gen: self.database_generation,
            tag_set: tag_set.clone(),
            count,
        });
        Ok(count)
    }

    pub fn works_list(&mut self, range: Range<usize>, tag_set: &TagSet) -> Result<Vec<Work>> {
        if let Some(cache) = self.works_list_cache.as_ref()
            && cache.db_gen == self.database_generation
            && cache.range == range
            && &cache.tag_set == tag_set
        {
            return Ok(cache.works.clone());
        }
        let works = self.pool.works_list(range.clone(), tag_set)?;
        let out = works.clone();
        self.works_list_cache = Some(WorksListCache {
            db_gen: self.database_generation,
            range,
            tag_set: tag_set.clone(),
            works,
        });
        Ok(out)
    }

    pub fn lookup_work(&mut self, work_id: i64) -> Result<Work> {
        if let Some(cache) = self.works_lookup_cache.as_ref()
            && cache.db_gen == self.database_generation
            && cache.work_id == work_id
        {
            return Ok(cache.work.clone());
        }
        let work = self.pool.lookup_work(work_id)?;
        let out = work.clone();
        self.works_lookup_cache = Some(WorksLookupCache {
            db_gen: self.database_generation,
            work_id,
            work,
        });
        Ok(out)
    }

    pub fn lookup_work_at_offset(&self, offset: usize, tag_set: &TagSet) -> Result<Work> {
        self.pool.lookup_work_at_offset(offset, tag_set)
    }
}
