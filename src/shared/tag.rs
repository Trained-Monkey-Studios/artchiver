use crate::db::writer::DbWriteHandle;
use crate::db::{
    models::tag::{DbTag, TagId},
    models::work::DbWork,
};
use crate::plugin::host::PluginHost;
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

pub enum TagRefresh {
    NoneNeeded,
    Favorites,
    NeedRefresh(TagId),
}

#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TagSet {
    enabled: HashSet<TagId>,
    disabled: HashSet<TagId>,

    last_fetched: Option<TagId>,
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

    pub fn get_best_refresh(&mut self, tags: Option<&HashMap<TagId, DbTag>>) -> TagRefresh {
        // We don't have any selection, so get favorites instead.
        if self.enabled.is_empty() {
            self.disabled.clear();
            self.last_fetched = None;
            return TagRefresh::Favorites;
        }

        // We already have it in the last-fetched set.
        if let Some(prior) = self.last_fetched
            && self.enabled.contains(&prior)
        {
            return TagRefresh::NoneNeeded;
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
            return TagRefresh::NeedRefresh(*min);
        }

        // Note: we know there is a first enabled tag because we checked above for empty.
        trace!("Fetch the first enabled tag");
        self.last_fetched = self.enabled.iter().next().copied();
        TagRefresh::NeedRefresh(self.last_fetched.expect("checked for none"))
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

    pub fn equals_single_tag(&self, tag: &DbTag) -> bool {
        self.disabled.is_empty() && self.enabled.len() == 1 && self.enabled.contains(&tag.id())
    }

    pub fn last_fetched(&self) -> Option<TagId> {
        self.last_fetched
    }

    pub fn enabled(&self) -> impl Iterator<Item = TagId> {
        self.enabled.iter().copied()
    }

    pub fn enabled_vec(&self) -> Vec<TagId> {
        self.enabled.iter().copied().collect()
    }

    pub fn disabled(&self) -> impl Iterator<Item = TagId> {
        self.disabled.iter().copied()
    }

    pub fn ui(&mut self, tags: &HashMap<TagId, DbTag>, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        let mut remove = None;
        for enabled in self.enabled() {
            if let Some(tag) = tags.get(&enabled) {
                let fav_icon = if tag.favorite() { "✨" } else { "" };
                let hid_icon = if tag.hidden() { "🗑" } else { "" };
                if ui
                    .button(format!("+{}{fav_icon}{hid_icon}", tag.name()))
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

        let mut unselect = None;
        for disabled in self.disabled() {
            if let Some(tag) = tags.get(&disabled) {
                let fav_icon = if tag.favorite() { "✨" } else { "" };
                let hid_icon = if tag.hidden() { "🗑" } else { "" };
                if ui
                    .button(format!("-{}{fav_icon}{hid_icon}", tag.name()))
                    .on_hover_text("Unselect negative filter")
                    .clicked()
                {
                    unselect = Some(disabled);
                }
            }
        }
        if let Some(unselect) = unselect {
            if let Some(tag) = tags.get(&unselect) {
                self.unselect(tag);
                changed = true;
            }
        }

        if !self.is_empty() {
            if ui.button("x").clicked() {
                self.clear();
                changed = true;
            }
        } else {
            ui.label("Favorites");
        }
        changed
    }

    pub fn tag_row_ui(
        &mut self,
        tag: &DbTag,
        host: &mut PluginHost,
        db_write: &DbWriteHandle,
        ui: &mut egui::Ui,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            let status = self.status(tag);
            let is_eq = self.equals_single_tag(tag);

            let prior_spacing = ui.style().spacing.item_spacing.x;
            ui.style_mut().spacing.item_spacing.x = 0.0;
            if ui
                .add(egui::Button::new("✔").small().selected(is_eq))
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
                if status.enabled() {
                    self.unselect(tag);
                } else {
                    self.enable(tag);
                }
                changed = true;
            }
            if ui
                .add(egui::Button::new("-").small().selected(status.disabled()))
                .on_hover_text("filter on negation")
                .clicked()
            {
                if status.disabled() {
                    self.unselect(tag);
                } else {
                    self.disable(tag);
                }
                changed = true;
            }
            let fav_text = if tag.favorite() { "★" } else { "☆" };
            if ui
                .small_button(fav_text)
                .on_hover_text("toggle favorite status")
                .clicked()
            {
                db_write
                    .set_tag_favorite(tag.id(), !tag.favorite())
                    .expect("database closed");
            }

            ui.label("  ");

            let content = if let Some(local_count) = tag.local_count() {
                format!(
                    "{} ({} of {})",
                    tag.name(),
                    local_count,
                    tag.network_count()
                )
            } else {
                format!("{} ([loading...] of {})", tag.name(), tag.network_count())
            };
            if status.disabled() {
                ui.label(egui::RichText::new(content).strikethrough());
            } else if status.enabled() {
                ui.label(egui::RichText::new(content).strong());
            } else {
                ui.label(content);
            }

            ui.label("  ");

            if ui
                .small_button("⟳")
                .on_hover_text("refresh works")
                .clicked()
            {
                host.refresh_works_for_tag(tag).ok();
            }
            if ui
                .add_enabled(tag.wiki_url().is_some(), egui::Button::new("🔗").small())
                .on_hover_text("go to wiki")
                .clicked()
            {
                let url = tag.wiki_url().expect("checked by egui");
                open::that(url).ok();
            }
            if ui.small_button("🗑").on_hover_text("hide tag").clicked() {
                db_write
                    .set_tag_hidden(tag.id(), !tag.hidden())
                    .expect("Database closed");
            }

            ui.style_mut().spacing.item_spacing.x = prior_spacing;
        });
        changed
    }
}
