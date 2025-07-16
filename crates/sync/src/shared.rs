use crate::progress::Progress;
use artchiver_sdk::*;
use itertools::Itertools;
use rusqlite::types::Value;
use std::{collections::HashSet, fmt, rc::Rc};

#[derive(Clone, Debug)]
pub(crate) enum PluginRequest {
    Shutdown,
    ApplyConfiguration { config: Vec<(String, String)> },
    RefreshTags,
    RefreshWorksForTag { tag: String },
}

#[derive(Clone, Debug)]
pub(crate) enum PluginResponse {
    PluginInfo(PluginMetadata),
    Progress(Progress),
    Message(String),
    Trace(String),
    DatabaseChanged,
}

pub enum TagStatus {
    Enabled,
    Disabled,
    Unselected,
}

impl TagStatus {
    pub fn enabled(&self) -> bool {
        matches!(self, TagStatus::Enabled)
    }

    pub fn disabled(&self) -> bool {
        matches!(self, TagStatus::Disabled)
    }
}

#[derive(Default, Debug)]
pub struct TagSet {
    enabled: HashSet<String>,
    disabled: HashSet<String>,
}

impl TagSet {
    pub fn status(&self, tag: &str) -> TagStatus {
        if self.enabled.contains(tag) {
            assert!(!self.disabled.contains(tag));
            TagStatus::Enabled
        } else if self.disabled.contains(tag) {
            TagStatus::Disabled
        } else {
            TagStatus::Unselected
        }
    }

    pub fn enable(&mut self, tag: &str) {
        self.enabled.insert(tag.to_owned());
        self.disabled.remove(tag);
    }

    pub fn unselect(&mut self, tag: &str) {
        self.enabled.remove(tag);
        self.disabled.remove(tag);
    }

    pub fn disable(&mut self, tag: &str) {
        self.enabled.remove(tag);
        self.disabled.insert(tag.to_owned());
    }

    // Build the enabled set into a vector suitable for passing to Rusqlites rarray function
    // e.g. for use with an SQL "IN" clause.
    pub fn enabled_rarray(&self) -> Rc<Vec<Value>> {
        Rc::new(self.enabled.iter().cloned().map(Value::from).collect())
    }
}

impl fmt::Display for TagSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enabled.is_empty() && self.disabled.is_empty() {
            return write!(f, "Select Tags to Show Matching Works");
        }
        let enabled = self.enabled.iter().map(|v| ('+', v.as_str()));
        let disabled = self.disabled.iter().map(|v| ('-', v.as_str()));
        let both = enabled.chain(disabled).sorted_by(|a, b| a.1.cmp(b.1));
        let mut out = String::new();
        for (i, (c, t)) in both.enumerate() {
            if i != 0 {
                out.push(' ');
            }
            out.push(c);
            out.push_str(t);
        }
        write!(f, "{out}")
    }
}
