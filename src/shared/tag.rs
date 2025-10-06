use crate::{
    db::{
        models::{
            tag::{DbTag, TagId},
            work::DbWork,
        },
        writer::DbWriteHandle,
    },
    plugin::host::PluginHost,
    ux::tutorial::{Tutorial, TutorialStep},
};
use itertools::Itertools as _;
use log::{trace, warn};
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
    NeedReproject,
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

    pub fn force_refresh(&mut self) {
        // Enable/disable/unselect/etc may change the sets we're looking at. This is called
        // when information is updated on the _DB_ side rather than on the visibility side,
        // so we need to set the refresh state to "dunno" so we will refresh, when asked.
        self.changed = true;
        self.last_fetched = None;
    }

    // This needs to be called each frame and the TagRefresh responded to in the works view.
    pub fn get_best_refresh(&mut self, tags: Option<&HashMap<TagId, DbTag>>) -> TagRefresh {
        if !self.changed {
            return TagRefresh::NoneNeeded;
        }
        self.changed = false;

        // We don't have any selection, so get favorites instead.
        if self.enabled.is_empty() {
            self.disabled.clear();
            self.last_fetched = None;
            return TagRefresh::Favorites;
        }

        // Even though the selected tags changed, we already fetched a superset tag.
        if let Some(prior) = self.last_fetched
            && self.enabled.contains(&prior)
        {
            return TagRefresh::NeedReproject;
        }

        // Find the tag with the smallest local-count in `enabled` to fetch.
        if let Some(tags) = tags {
            if let Some(min) = self
                .enabled
                .iter()
                .min_by_key(|t| tags.get(t).map(|t| t.local_count()))
            {
                trace!(
                    "Fetching the smallest local tag: {}",
                    tags.get(min).expect("checked").name()
                );
                self.last_fetched = Some(*min);
                return TagRefresh::NeedRefresh(*min);
            } else if let Some(min) = self
                .enabled
                .iter()
                .min_by_key(|t| tags.get(t).map(|t| t.network_count()))
            {
                // Note: fall back to the network counts if we haven't fully loaded yet.
                warn!(
                    "Falling back to fetch the smallest network tag: {}",
                    tags.get(min).expect("checked").name()
                );
                self.last_fetched = Some(*min);
                return TagRefresh::NeedRefresh(*min);
            }
        }

        // We're early enough in startup that we haven't even fetched the tags list yet;
        // however, we know there is at least one tag because we checked above for empty.
        warn!("Falling back to fetching the first enabled tag");
        self.last_fetched = self.enabled.iter().next().copied();
        TagRefresh::NeedRefresh(self.last_fetched.expect("checked for none"))
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

    pub fn ui(&mut self, tags: &HashMap<TagId, DbTag>, ui: &mut egui::Ui) {
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
        if let Some(remove) = remove
            && let Some(tag) = tags.get(&remove)
        {
            self.unselect(tag);
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
        if let Some(unselect) = unselect
            && let Some(tag) = tags.get(&unselect)
        {
            self.unselect(tag);
        }

        if !self.is_empty() {
            if ui.button("x").clicked() {
                self.clear();
            }
        } else {
            ui.label("Favorites");
        }
    }

    pub fn tag_row_ui(
        &mut self,
        tag: &DbTag,
        host: &mut PluginHost,
        db_write: &DbWriteHandle,
        ui: &mut egui::Ui,
        tutorial: &mut Tutorial<'_>,
    ) {
        ui.horizontal(|ui| {
            let status = self.status(tag);
            let is_eq = self.equals_single_tag(tag);

            let prior_spacing = ui.style().spacing.item_spacing.x;
            ui.style_mut().spacing.item_spacing.x = 0.0;
            if tutorial
                .add_step(
                    TutorialStep::TagsViewGeneral,
                    ui,
                    egui::Button::new("✔")
                        .small()
                        .selected(is_eq)
                        .corner_radius(egui::CornerRadius {
                            nw: 6,
                            sw: 6,
                            ne: 0,
                            se: 0,
                        }),
                )
                .on_hover_text("replace filter")
                .clicked()
            {
                self.clear();
                self.enable(tag);
            }
            if tutorial
                .add_step(
                    TutorialStep::TagsViewAdd,
                    ui,
                    egui::Button::new("+")
                        .small()
                        .selected(status.enabled())
                        .corner_radius(egui::CornerRadius::same(0)),
                )
                .on_hover_text("add filter")
                .clicked()
            {
                if status.enabled() {
                    self.unselect(tag);
                } else {
                    self.enable(tag);
                }
            }
            if tutorial
                .add_step(
                    TutorialStep::TagsViewSubtract,
                    ui,
                    egui::Button::new("-")
                        .small()
                        .selected(status.disabled())
                        .corner_radius(egui::CornerRadius::same(0)),
                )
                .on_hover_text("filter on negation")
                .clicked()
            {
                if status.disabled() {
                    self.unselect(tag);
                } else {
                    self.disable(tag);
                }
            }
            let fav_text = if tag.favorite() { "★" } else { "☆" };
            if ui
                .add(
                    egui::Button::new(fav_text)
                        .small()
                        .corner_radius(egui::CornerRadius::same(0)),
                )
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

            if tutorial
                .add_step(
                    TutorialStep::TagsRefresh,
                    ui,
                    egui::Button::new("⟳").small(),
                )
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
    }
}
