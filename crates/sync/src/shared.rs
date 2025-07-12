use crate::progress::Progress;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginMetadata {
    name: String,
    version: String,
    description: String,
}

impl PluginMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PluginRequest {
    Shutdown,
    RefreshTags,
}

#[derive(Clone, Debug)]
pub(crate) enum PluginResponse {
    PluginInfo(PluginMetadata),
    Progress(Progress),
    Message(String),
    DatabaseChanged,
}
