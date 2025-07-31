use crate::sync::db::models::tag::{DbTag, TagId};
use crate::sync::db::models::work::DbWork;
use itertools::Itertools as _;
use log::trace;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    enabled: HashSet<TagId>,
    disabled: HashSet<TagId>,

    #[serde(skip)]
    last_fetched: Option<TagId>,
    #[serde(skip)]
    changed: bool,
}

impl TagSet {
    pub fn matches(&self, work: &DbWork) -> bool {
        self.enabled.iter().all(|t| work.tags().contains(t))
            && !self.disabled.iter().any(|t| work.tags().contains(t))
    }

    pub fn status(&self, tag: &DbTag) -> TagStatus {
        if self.enabled.contains(&tag.id()) {
            assert!(!self.disabled.contains(&tag.id()), "tag in both sets");
            TagStatus::Enabled
        } else if self.disabled.contains(&tag.id()) {
            TagStatus::Disabled
        } else {
            TagStatus::Unselected
        }
    }

    pub fn get_best_refresh(&mut self, tags: Option<&HashMap<TagId, DbTag>>) -> Option<TagId> {
        // We don't have any selection, so nothing to fetch.
        if self.enabled.is_empty() {
            return None;
        }
        // We already have it in the last-fetched set.
        if let Some(prior) = self.last_fetched
            && self.enabled.contains(&prior)
        {
            trace!("Last fetched tag is still enabled, skipping");
            return None;
        }

        // Find the tag with the smallest local-count in `enabled` to fetch.
        if let Some(tags) = tags
            && let Some(min) = self
                .enabled
                .iter()
                .min_by_key(|t| tags.get(t).map(|t| t.local_count()))
        {
            trace!(
                "Should fetch the smallest tag: {}",
                tags.get(min).expect("checked").name()
            );
            self.last_fetched = Some(*min);
        } else {
            trace!("Fetch the first enabled tag");
            self.last_fetched = self.enabled.iter().next().copied();
        }

        self.last_fetched
    }

    pub fn reset_changed(&mut self) -> bool {
        let out = self.changed;
        self.changed = false;
        out
    }

    pub fn enable(&mut self, tag: &DbTag) {
        self.enabled.insert(tag.id());
        self.disabled.remove(&tag.id());
        self.changed = true;
    }

    pub fn unselect(&mut self, tag: &DbTag) {
        self.enabled.remove(&tag.id());
        self.disabled.remove(&tag.id());
        self.changed = true;
    }

    pub fn disable(&mut self, tag: &DbTag) {
        self.enabled.remove(&tag.id());
        self.disabled.insert(tag.id());
        self.changed = true;
    }

    pub fn clear(&mut self) {
        self.enabled.clear();
        self.disabled.clear();
        self.changed = true;
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty()
    }

    pub fn enabled(&self) -> impl Iterator<Item = TagId> {
        self.enabled.iter().copied()
    }

    pub fn enabled_vec(&self) -> Vec<TagId> {
        self.enabled.iter().copied().collect()
    }

    pub fn ui(&mut self, tags: &HashMap<TagId, DbTag>, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut remove = None;
        for enabled in self.enabled() {
            if let Some(tag) = tags.get(&enabled) {
                if ui
                    .button(format!("+{}", tag.name()))
                    .on_hover_text("Remove Filter")
                    .clicked()
                {
                    remove = Some(enabled);
                }
            }
        }
        if let Some(remove) = remove {
            if let Some(tag) = tags.get(&remove) {
                self.unselect(tag);
                changed = true;
            }
        }
        if ui.button("x").clicked() {
            self.clear();
            changed = true;
        }
        changed
    }

    pub fn tag_row_ui(&mut self, tag: &DbTag, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            let prior_spacing = ui.style().spacing.item_spacing.x;
            ui.style_mut().spacing.item_spacing.x = 0.0;
            let status = self.status(tag);
            if ui
                .add(egui::Button::new("✔").small().selected(status.enabled()))
                .on_hover_text("replace filter")
                .clicked()
            {
                self.clear();
                self.enable(tag);
                changed = true;
            }
            if ui
                .add(egui::Button::new("+").small().selected(status.enabled()))
                .on_hover_text("add filter")
                .clicked()
            {
                self.enable(tag);
                changed = true;
            }
            if ui
                .add(egui::Button::new(" ").small())
                .on_hover_text("remove filter")
                .clicked()
            {
                self.unselect(tag);
                changed = true;
            }
            if ui
                .add(egui::Button::new("x").small().selected(status.disabled()))
                .on_hover_text("filter on negation")
                .clicked()
            {
                self.disable(tag);
                changed = true;
            }
            ui.label("  ");
            ui.label(tag.name());
            ui.style_mut().spacing.item_spacing.x = prior_spacing;
        });
        changed
    }
}
