use crate::shared::progress::Progress;
use artchiver_sdk::PluginMetadata;

#[derive(Clone, Debug)]
pub enum PluginRequest {
    Shutdown,
    ApplyConfiguration { config: Vec<(String, String)> },
    RefreshTags,
    RefreshWorksForTag { tag: String },
}

#[derive(Clone, Debug)]
pub enum PluginResponse {
    PluginInfo(PluginMetadata),
    Progress(Progress),
    Message(String),
    Trace(String),
    DatabaseChanged,
}
