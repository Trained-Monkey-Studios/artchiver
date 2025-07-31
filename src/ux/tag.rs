use crate::{
    shared::{tag::TagSet, update::DataUpdate},
    sync::{
        db::{
            model::OrderDir,
            models::tag::{DbTag, TagId},
            reader::DbReadHandle,
        },
        plugin::host::PluginHost,
    },
};
use itertools::Itertools as _;
use log::trace;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TagSortCol {
    #[default]
    Name,
    LocalCount,
    NetworkCount,
}

impl TagSortCol {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut selected = match self {
            Self::Name => 0,
            Self::LocalCount => 1,
            Self::NetworkCount => 2,
        };
        let labels = ["Name", "Works Downloaded", "Total Works"];
        egui::ComboBox::new("tag_order_column", "Column")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, labels.len(), |i| labels[i]);
        *self = match selected {
            0 => Self::Name,
            1 => Self::LocalCount,
            2 => Self::NetworkCount,
            _ => panic!("invalid column selected"),
        };
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TagOrder {
    column: TagSortCol,
    order: OrderDir,
}

impl TagOrder {
    pub fn new(column: TagSortCol, order: OrderDir) -> Self {
        Self { column, order }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.column.ui(ui);
        self.order.ui("tags", ui);
    }
}

/// Tag caching strategy:
///
/// Plan for O(100-500k) tags -- the approximate size of the English vocabulary with misspelling --
/// so we just store all of them in memory and refresh from the database after a refresh finishes.
///
/// When a filter is applied, we map over the full set of tags into a filtered tag
/// set. We do this immediately: todo: check if we can do live updates here?
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UxTag {
    // A substring matcher over tag names
    name_filter: String,
    source_filter: Option<String>,
    order: TagOrder,

    #[serde(skip, default)]
    tag_all: Option<HashMap<TagId, DbTag>>,

    // Ordered subset of DbTag id's to actually draw each frame.
    #[serde(skip, default)]
    tag_filtered: Vec<TagId>,
}

impl UxTag {
    pub fn startup(&mut self, db: &DbReadHandle) {
        trace!("Starting up tag UX");

        // Reload tags from DB at startup so we don't have to put them in the app state.
        db.get_tags();
    }

    pub fn handle_updates(&mut self, db: &DbReadHandle, updates: &[DataUpdate]) {
        for update in updates {
            match update {
                DataUpdate::InitialTags(tags) => {
                    trace!("Received {} initial tags", tags.len());
                    self.tag_all = Some(tags.clone());
                    self.reproject_tags();
                }
                DataUpdate::TagsLocalCounts(counts) => {
                    if let Some(tags) = &mut self.tag_all {
                        for (tag_id, count) in counts {
                            if let Some(tag) = tags.get_mut(tag_id) {
                                tag.set_local_count(*count);
                            }
                        }
                    }
                    self.reproject_tags();
                }
                DataUpdate::TagsWereRefreshed => {
                    self.tag_all = None;
                    self.tag_filtered = vec![];
                    db.get_tags();
                }
                DataUpdate::WorksWereUpdatedForTag { .. } => {
                    db.get_tag_local_counts();
                }
                _ => {}
            }
        }
    }

    pub fn tags(&self) -> Option<&HashMap<TagId, DbTag>> {
        self.tag_all.as_ref()
    }

    fn reproject_tags(&mut self) {
        if let Some(tags) = &self.tag_all {
            self.tag_filtered = tags
                .iter()
                .filter(|(_id, t)| t.name().contains(&self.name_filter))
                // include only selected plugin sources in the tags list response
                .filter(|(_, t)| {
                    self.source_filter.is_none()
                        || t.sources()
                            .contains(self.source_filter.as_ref().expect("checked"))
                })
                // FIXME: add UX and filter to hide, browse hidden tags, and unhide
                // .filter(|(_, t)| t.hidden)
                .sorted_by(|(_, a), (_, b)| {
                    let ord = match self.order.column {
                        TagSortCol::Name => a.name().cmp(b.name()),
                        TagSortCol::LocalCount => a.local_count().cmp(&b.local_count()),
                        TagSortCol::NetworkCount => a.network_count().cmp(&b.network_count()),
                    };
                    match self.order.order {
                        OrderDir::Asc => ord,
                        OrderDir::Desc => ord.reverse(),
                    }
                })
                .map(|(id, _)| *id)
                .collect();
        }
    }

    // Note: no pool so no way to block
    pub fn ui(&mut self, tag_set: &mut TagSet, host: &mut PluginHost, ui: &mut egui::Ui) {
        if self.tag_all.is_none() {
            ui.spinner();
            return;
        }

        // Filter and view bar
        ui.horizontal(|ui| {
            if ui.text_edit_singleline(&mut self.name_filter).changed() {
                self.reproject_tags();
            }
            if ui.button("x").clicked() {
                self.name_filter.clear();
                self.reproject_tags();
            }
            ui.label(format!("({})", self.tag_filtered.len()));

            let mut selected = 0usize;
            let mut options = host.plugins().map(|p| p.name()).collect::<Vec<_>>();
            options.insert(0, "All".to_owned());
            if let Some(source) = self.source_filter.as_deref() {
                if let Some((offset, _)) = options.iter().find_position(|v| v == &source) {
                    selected = offset;
                }
            }
            egui::ComboBox::new("tag_filter_sources", "Source")
                .wrap_mode(egui::TextWrapMode::Truncate)
                .show_index(ui, &mut selected, options.len(), |i| &options[i]);
            if options[selected] == "All" {
                self.source_filter = None;
                self.reproject_tags();
            } else {
                self.source_filter = Some(options[selected].clone());
                self.reproject_tags();
            }
        });
        // Sorting
        ui.horizontal(|ui| {
            self.order.ui(ui);
        });

        let all_tags = self.tag_all.as_ref().expect("no tags after check");
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, self.tag_filtered.len(), |ui, row_range| {
                egui::Grid::new("tag_grid")
                    .num_columns(1)
                    .spacing([0., 0.])
                    .min_col_width(0.)
                    .show(ui, |ui| {
                        for tag_id in &self.tag_filtered[row_range] {
                            let tag = all_tags.get(tag_id).expect("missing tag");
                            let status = tag_set.status(tag);
                            if ui
                                .add(egui::Button::new("✔").small().selected(status.enabled()))
                                .on_hover_text("replace filter")
                                .clicked()
                            {
                                tag_set.clear();
                                tag_set.enable(tag);
                            }
                            if ui
                                .add(egui::Button::new("+").small().selected(status.enabled()))
                                .on_hover_text("add filter")
                                .clicked()
                            {
                                tag_set.enable(tag);
                            }
                            if ui
                                .add(egui::Button::new(" ").small())
                                .on_hover_text("remove filter")
                                .clicked()
                            {
                                tag_set.unselect(tag);
                            }
                            if ui
                                .add(egui::Button::new("x").small().selected(status.disabled()))
                                .on_hover_text("filter on negation")
                                .clicked()
                            {
                                tag_set.disable(tag);
                            }
                            ui.label("   ");
                            if ui.button("⟳").on_hover_text("refresh works").clicked() {
                                host.refresh_works_for_tag(tag).ok();
                            }
                            ui.label("   ");
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
                            ui.end_row();
                        }
                    });
            });
    }
}
