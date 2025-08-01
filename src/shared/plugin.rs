use parking_lot::Mutex;
use std::{fmt, sync::Arc};

#[derive(Clone, Debug)]
pub enum PluginRequest {
    ApplyConfiguration { config: Vec<(String, String)> },
    RefreshTags,
    RefreshWorksForTag { tag: String },
    Shutdown,
}

impl fmt::Display for PluginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplyConfiguration { .. } => write!(f, "Apply Configuration"),
            Self::RefreshTags => write!(f, "Refresh Tags"),
            Self::RefreshWorksForTag { tag } => write!(f, "Get Works for Tag {tag}"),
            Self::Shutdown => write!(f, "Shutdown"),
        }
    }
}

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
