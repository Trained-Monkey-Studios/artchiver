use crate::{
    db::models::{
        plugin::DbPlugin,
        tag::{DbTag, TagId},
        work::{DbWork, WorkId},
    },
    shared::progress::{Progress, UpdateSource},
};
use artchiver_sdk::PluginMetadata;
use log::Level;
use std::{collections::HashMap, path::PathBuf};

pub enum DataUpdate {
    // Provides information about the plugin back to PluginHost for display in the UX
    PluginInfo {
        source: PathBuf,
        record: DbPlugin,
        metadata: PluginMetadata,
    },

    // Notifies the UX that a block of tags from a provider has been upserted. The UX should
    // discard it's cached tags and re-query.
    TagsWereRefreshed,

    // Notifies the UX that a block of work metadata has been upserted into the DB. If the UX
    // has cached any data for the given tag, it should drop it and re-query the DB. Note that
    // this doesn't include progress on downloading any of the images associated with those works.
    WorksWereUpdatedForTag {
        for_tag: String,
    },

    // Notify the UX that a specific work's image downloads have completed and it can now present
    // those works to the user.
    WorkDownloadCompleted {
        id: WorkId,
        preview_path: String,
        screen_path: String,
        archive_path: Option<String>,
    },

    // Status change for favorite and hidden flags.
    WorkFavoriteStatusChanged {
        work_id: WorkId,
        favorite: bool,
    },
    WorkHiddenStatusChanged {
        work_id: WorkId,
        hidden: bool,
    },
    TagFavoriteStatusChanged {
        tag_id: TagId,
        favorite: bool,
    },
    TagHiddenStatusChanged {
        tag_id: TagId,
        hidden: bool,
    },

    // Notify the PluginHost that the source has completed a task and needs to be fed new work.
    CompletedTask {
        source: UpdateSource,
    },

    // Revisit these:
    Progress {
        source: UpdateSource,
        progress: Progress,
    },
    Log {
        source: UpdateSource,
        level: Level,
        message: String,
    },

    // Fulfills a request by the UX to get the current list of tags.
    InitialTags(HashMap<TagId, DbTag>),
    TagsLocalCounts(Vec<(TagId, u64)>),

    // Fulfills a request by the UX to get the current list of works for a tag.
    FetchWorksComplete {
        tag_id: Option<TagId>,
        works: HashMap<WorkId, DbWork>,
    },
}
