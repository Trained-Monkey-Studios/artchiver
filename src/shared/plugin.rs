use crate::shared::progress::Progress;
use artchiver_sdk::PluginMetadata;
use log::Level;
use std::fmt;

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

#[derive(Clone, Debug)]
pub enum PluginResponse {
    // Startup sequence
    PluginInfo(PluginMetadata),

    // Informational to keep the UX alive.
    Progress(Progress),
    Log(Level, String),
    DatabaseChanged,

    // Must be returned each time the plugin completes some work; otherwise the plugin will not be
    // fed more requests to work on.
    CompletedTask,
}
