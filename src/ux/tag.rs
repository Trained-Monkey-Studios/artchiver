use crate::ux::tutorial::{Tutorial, TutorialStep};
use crate::{
    db::{
        model::OrderDir,
        models::tag::{DbTag, TagId},
        reader::DbReadHandle,
        writer::DbWriteHandle,
    },
    plugin::host::PluginHost,
    shared::{tag::TagSet, update::DataUpdate},
};
use artchiver_sdk::TagKind;
use itertools::Itertools as _;
use log::trace;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap};

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TagSourceFilter {
    source: Option<String>,
}

impl TagSourceFilter {
    pub fn ui(&mut self, host: &PluginHost, ui: &mut egui::Ui) -> bool {
        let mut selected = 0usize;
        let mut options = host.plugins().map(|p| p.name()).collect::<Vec<_>>();
        options.insert(0, "All".to_owned());
        options.push("Hidden".to_owned());
        if let Some(source) = self.source.as_deref()
            && let Some((offset, _)) = options.iter().find_position(|v| v == &source)
        {
            selected = offset;
        }
        let prior = selected;
        egui::ComboBox::new("tag_filter_sources", "Source")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, options.len(), |i| &options[i]);
        if prior != selected {
            if options[selected] == "All" {
                self.source = None;
            } else {
                self.source = Some(options[selected].clone());
            }
            return true;
        }
        false
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TagKindFilter {
    kind: Option<TagKind>,
}

impl TagKindFilter {
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut selected = match self.kind {
            None => 0,
            Some(TagKind::Default) => 1,
            Some(TagKind::Character) => 2,
            Some(TagKind::Copyright) => 3,
            Some(TagKind::Location) => 4,
            Some(TagKind::Meta) => 5,
            Some(TagKind::School) => 6,
            Some(TagKind::Series) => 7,
            Some(TagKind::Style) => 8,
            Some(TagKind::Technique) => 9,
            Some(TagKind::Theme) => 10,
        };
        const LABELS: [&str; 11] = [
            "All",
            "Default",
            "Character",
            "Copyright",
            "Location",
            "Meta",
            "School",
            "Series",
            "Style",
            "Technique",
            "Theme",
        ];
        let prior = selected;
        egui::ComboBox::new("tag_filter_kind", "Kind")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, LABELS.len(), |i| LABELS[i]);
        self.kind = match selected {
            0 => None,
            1 => Some(TagKind::Default),
            2 => Some(TagKind::Character),
            3 => Some(TagKind::Copyright),
            4 => Some(TagKind::Location),
            5 => Some(TagKind::Meta),
            6 => Some(TagKind::School),
            7 => Some(TagKind::Series),
            8 => Some(TagKind::Style),
            9 => Some(TagKind::Technique),
            10 => Some(TagKind::Theme),
            _ => panic!("invalid tag kind selected"),
        };
        prior != selected
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
    source_filter: TagSourceFilter,
    kind_filter: TagKindFilter,
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
                    // Note: whenever we fetch more works, the tag counts on unrelated tags will
                    //       change. We need to do a full recount.
                    db.get_tag_local_counts();
                }
                DataUpdate::TagFavoriteStatusChanged { tag_id, favorite } => {
                    if let Some(tags) = &mut self.tag_all
                        && let Some(tag) = tags.get_mut(tag_id)
                    {
                        tag.set_favorite(*favorite);
                        self.reproject_tags();
                    }
                }
                DataUpdate::TagHiddenStatusChanged { tag_id, hidden } => {
                    if let Some(tags) = &mut self.tag_all
                        && let Some(tag) = tags.get_mut(tag_id)
                    {
                        tag.set_hidden(*hidden);
                        self.reproject_tags();
                    }
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
                // Apply the direct name filter, case-insensitive
                .filter(|(_id, t)| {
                    t.name()
                        .to_lowercase()
                        .contains(&self.name_filter.to_lowercase())
                })
                // include only selected plugin sources in the tags list response
                .filter(|(_, t)| {
                    self.source_filter.source.is_none() // All
                        || (self.source_filter.source.as_deref() == Some("Hidden") && t.hidden())
                        || t.sources().contains(self.source_filter.source.as_deref().expect("checked"))
                })
                // include only tags with the selected kind
                .filter(|(_, t)| {
                    self.kind_filter.kind.is_none() ||
                        self.kind_filter.kind == Some(t.kind())
                })
                .sorted_by(
                    |(_, a), (_, b)| match a.favorite().cmp(&b.favorite()).reverse() {
                        Ordering::Equal => {
                            let inner = match self.order.column {
                                TagSortCol::Name => a.name().cmp(b.name()),
                                TagSortCol::LocalCount => {
                                    match a.local_count().cmp(&b.local_count()) {
                                        Ordering::Equal => a.name().cmp(b.name()),
                                        v => v,
                                    }
                                }
                                TagSortCol::NetworkCount => {
                                    match a.network_count().cmp(&b.network_count()) {
                                        Ordering::Equal => a.name().cmp(b.name()),
                                        v => v,
                                    }
                                }
                            };
                            match self.order.order {
                                OrderDir::Asc => inner,
                                OrderDir::Desc => inner.reverse(),
                            }
                        }
                        v => v,
                    },
                )
                .map(|(id, _)| *id)
                .collect();
        }
    }

    pub fn ui(
        &mut self,
        tag_set: &mut TagSet,
        host: &mut PluginHost,
        mut tutorial: Tutorial<'_>,
        db_write: &DbWriteHandle,
        ui: &mut egui::Ui,
    ) {
        if self.tags().is_none() || self.tags().expect("checked").is_empty() {
            // Show an apologetic message while the plugin does its work.
            if tutorial.step() == TutorialStep::TagsIntro {
                tutorial.frame(ui, |ui, _tutorial| {
                    ui.heading("About Tags").scroll_to_me(None);
                    ui.separator();
                    ui.label("Please be patient while the tags load; it may take a few seconds, depending on your network speed. The progress bar next to the plugin will tell you when its almost done.");
                    ui.label("");
                    ui.label("In Artchiver, tags are how we find and browse artworks.");
                    ui.label("");
                    ui.label("The tags list can be quite long, so the tools at the top of this panel will allow us to filter and sort tags in various ways, once they show up.");
                    ui.label("");
                    ui.label("Below that, there will be a long list of tags, each of which has various controls to download and view artworks.");
                });
            }
            return;
        }

        // We have the tags now, so explain what to do next.
        if tutorial.step() == TutorialStep::TagsIntro {
            tutorial.frame(ui, |ui, tutorial| {
                ui.heading("About Tags").scroll_to_me(None);
                ui.separator();
                ui.label("In Artchiver, tags are how we find and browse artworks.");
                ui.label("");
                ui.label("The tags list can be quite long, so the tools at the top of this panel allow us to filter and sort tags in various ways.");
                ui.label("");
                ui.label("Below that, there should be a long list of tags, each of which has various controls to download and view artworks.");
                ui.label("");
                ui.label("Click the next button and we will go over some of those controls. We will be using the `` tag, but the same tools will work on any tag in this list.");
                ui.separator();
                if ui.button("Next").clicked() {
                    tutorial.next();
                }
            });
        }

        // Main textual filter bar
        ui.horizontal(|ui| {
            if ui.text_edit_singleline(&mut self.name_filter).changed() {
                self.reproject_tags();
            }
            if ui.button("x").clicked() {
                self.name_filter.clear();
                self.reproject_tags();
            }
            ui.label(format!("({})", self.tag_filtered.len()));
        });
        // Sub-filters bar
        ui.horizontal(|ui| {
            if self.source_filter.ui(host, ui) {
                self.reproject_tags();
            }
            if self.kind_filter.ui(ui) {
                self.reproject_tags();
            }
        });
        // Sorting bar
        ui.horizontal(|ui| {
            let prior = self.order;
            self.order.ui(ui);
            if prior != self.order {
                self.reproject_tags();
            }
        });

        let all_tags = self.tag_all.as_ref().expect("no tags after check");
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, self.tag_filtered.len(), |ui, row_range| {
                egui::Grid::new("tag_grid")
                    .num_columns(1)
                    .spacing([2., 2.])
                    .min_col_width(0.)
                    .show(ui, |ui| {
                        for tag_id in &self.tag_filtered[row_range] {
                            let tag = all_tags.get(tag_id).expect("missing tag");
                            // Note: change is captured internally
                            tag_set.tag_row_ui(tag, host, db_write, ui);
                            ui.end_row();
                        }
                    });
            });
    }
}
