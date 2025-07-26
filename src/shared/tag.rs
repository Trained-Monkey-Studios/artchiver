use crate::sync::db::work::DbWork;
use itertools::Itertools as _;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt, rc::Rc};

pub enum TagStatus {
    Enabled,
    Disabled,
    Unselected,
}

impl TagStatus {
    pub fn enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TagSet {
    enabled: HashSet<String>,
    disabled: HashSet<String>,
}

impl TagSet {
    pub fn matches(&self, work: &DbWork) -> bool {
        work.tags().all(|t| self.enabled.contains(t))
            && !work.tags().any(|t| self.disabled.contains(t))
    }

    pub fn status(&self, tag: &str) -> TagStatus {
        if self.enabled.contains(tag) {
            assert!(!self.disabled.contains(tag), "tag in both sets");
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

    pub fn clear(&mut self) {
        self.enabled.clear();
        self.disabled.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty()
    }

    pub fn enabled(&self) -> impl Iterator<Item = &String> {
        self.enabled.iter()
    }

    // Build the enabled set into a vector suitable for passing to Rusqlites rarray function
    // e.g. for use with an SQL "IN" clause.
    pub fn enabled_rarray(&self) -> Rc<Vec<Value>> {
        Rc::new(self.enabled.iter().cloned().map(Value::from).collect())
    }

    pub fn enabled_count(&self) -> usize {
        self.enabled.len()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut remove = None;
        for enabled in self.enabled() {
            if ui
                .button(format!("+{enabled}"))
                .on_hover_text("Remove Filter")
                .clicked()
            {
                remove = Some(enabled.to_owned());
            }
        }
        if let Some(tag) = remove {
            self.unselect(&tag);
            changed = true;
        }
        if ui.button("x").clicked() {
            self.clear();
            changed = true;
        }
        changed
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
