use crate::shared::progress::Progress;
use crate::sync::db::tag::DbTag;
use artchiver_sdk::PluginMetadata;
use log::Level;
use std::{
    path::PathBuf,
    collections::HashMap
};

pub enum DataUpdate {
    // Plugin messages
    TagsWereUpdated,
    WorksWereUpdatedForTag { tag: String },
    WorkDownloadCompleted { id: i64 },
    // Revisit these:
    PluginInfo { source: PathBuf, metadata: PluginMetadata },
    Progress(Progress),
    Log(Level, String),
    TagsRefreshed,
    CompletedTask,

    // DB Messages
    InitialTags(HashMap<i64, DbTag>),
    TagsLocalCounts(Vec<(i64, u64)>),
    TagsNetworkCounts(Vec<(i64, u64)>),
}
