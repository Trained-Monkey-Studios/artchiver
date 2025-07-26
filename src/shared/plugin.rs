use crate::{shared::progress::Progress, sync::db::tag::DbTag};
use artchiver_sdk::PluginMetadata;
use log::Level;
use parking_lot::Mutex;
use std::{fmt, sync::Arc};

#[derive(Clone, Debug)]
pub enum PluginRequest {
    Shutdown,
    ApplyConfiguration { config: Vec<(String, String)> },
    RefreshTags,
    RefreshWorksForTag { tag: String },
}

impl fmt::Display for PluginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shutdown => write!(f, "Shutdown"),
            Self::ApplyConfiguration { .. } => write!(f, "Apply Configuration"),
            Self::RefreshTags => write!(f, "Refresh Tags"),
            Self::RefreshWorksForTag { tag } => write!(f, "Get Works for Tag {tag}"),
        }
    }
}

// #[derive(Clone, Debug)]
// pub enum PluginResponse {
//     // Startup notifications /////////////////////////////
//     PluginInfo(PluginMetadata),
//
//     // Status notifications //////////////////////////////
//     Progress(Progress),
//     Log(Level, String),
//
//     // Data change notifications /////////////////////////
//     // Note: no extra data, since tag refreshes are enormous and rare
//     TagsRefreshed,
//
//     // Task Maintenance //////////////////////////////////
//     // Must be returned each time the plugin completes some work;
//     // otherwise the plugin will not be fed more requests to work on.
//     CompletedTask,
// }

#[derive(Clone, Debug, Default)]
pub struct PluginCancellation {
    signal: Arc<Mutex<bool>>,
}

impl PluginCancellation {
    pub fn cancel(&self) {
        *self.signal.lock() = true;
    }

    pub fn reset(&self) {
        *self.signal.lock() = false;
    }

    pub fn is_cancelled(&self) -> bool {
        *self.signal.lock()
    }
}
