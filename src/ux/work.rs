use crate::sync::db::sync::DbSyncHandle;
use crate::sync::db::tag::{DbTag, TagId};
use crate::{
    shared::{performance::PerfTrack, tag::TagSet, update::DataUpdate},
    sync::db::{
        model::OrderDir,
        reader::DbReadHandle,
        work::{DbWork, WorkId},
    },
};
use egui::{Key, Margin, Modifiers, Rect, Sense, SizeHint, Vec2, include_image};
use itertools::Itertools as _;
use log::{info, trace};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum WorkSize {
    // Thumbnail,
    Preview,
    Screen,
    // Archive
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkSortCol {
    #[default]
    Date,
}

impl WorkSortCol {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut selected = match self {
            Self::Date => 0,
        };
        let labels = ["Name", "Total Count"];
        egui::ComboBox::new("tag_order_column", "Column")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, labels.len(), |i| labels[i]);
        *self = match selected {
            0 => Self::Date,
            _ => panic!("invalid column selected"),
        };
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkOrder {
    column: WorkSortCol,
    order: OrderDir,
}

impl WorkOrder {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.column.ui(ui);
        self.order.ui("tags", ui);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkVisibility {
    #[default]
    All,
    Favorites,
    Hidden,
}

impl WorkVisibility {
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut selected = match self {
            Self::All => 0,
            Self::Favorites => 1,
            Self::Hidden => 2,
        };
        let labels = ["All", "Favorites", "Hidden"];
        egui::ComboBox::new("work_visibility", "Visibility")
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_index(ui, &mut selected, labels.len(), |i| labels[i]);
        let next = match selected {
            0 => Self::All,
            1 => Self::Favorites,
            2 => Self::Hidden,
            _ => panic!("invalid column selected"),
        };
        let changed = *self != next;
        *self = next;
        changed
    }
}

/// Work caching strategy:
///
/// Works are unbounded, but plan to scale to O(10-100M) works. This is too much for us to just
/// read everything, so we have to read blocks of the potentially visible set, based on what
/// is active in the tag set. Changing tag sets triggers a re-read of the database, but it would
/// be nice if we could just show everything if there are no tags.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UxWork {
    // Offset into work_filtered.
    selected: Option<usize>,
    thumb_size: f32,

    // Filter state for the works gallery
    tag_selection: TagSet,
    order: WorkOrder,
    showing: WorkVisibility,

    // Don't cache things that are too long or only last one frame
    #[serde(skip)]
    per_frame_work_upload_count: usize,

    // The cached set of works is everything selected by the tag_set.
    #[serde(skip)]
    work_matching_tag: Option<HashMap<WorkId, DbWork>>,

    #[serde(skip)]
    work_filtered: Vec<WorkId>,

    #[serde(skip)]
    data_dir: PathBuf,

    #[serde(skip, default = "LruCache::unbounded")]
    works_lru: LruCache<String, u32>,
}

impl Default for UxWork {
    fn default() -> Self {
        Self {
            selected: None,
            thumb_size: 128.,
            tag_selection: TagSet::default(),
            order: WorkOrder::default(),
            showing: WorkVisibility::default(),
            per_frame_work_upload_count: 0,
            work_matching_tag: None,
            work_filtered: Vec::new(),
            data_dir: PathBuf::new(),
            works_lru: LruCache::unbounded(),
        }
    }
}

impl UxWork {
    const LRU_CACHE_SIZE: usize = 500;
    const MAX_PER_FRAME_UPLOADS: usize = 3;

    pub fn startup(&mut self, data_dir: &Path, db: &DbReadHandle) {
        trace!("Starting up work UX");

        self.data_dir = data_dir.to_owned();
        if let Some(tag_id) = self.tag_selection.enabled().next() {
            db.get_works_for_tag(tag_id);
        } else if self.tag_selection.is_empty() {
            db.get_favorite_works();
        }
    }

    pub fn handle_updates(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db: &DbReadHandle,
        updates: &[DataUpdate],
    ) {
        for update in updates {
            match update {
                DataUpdate::FetchWorksComplete { works } => {
                    trace!("Received {} works", works.len());
                    self.work_matching_tag = Some(works.to_owned());
                    self.reproject_work();
                }
                DataUpdate::WorksWereUpdatedForTag { .. } => {
                    self.tags_changed(tags, db);
                }
                DataUpdate::WorkDownloadCompleted {
                    id,
                    preview_path,
                    screen_path,
                    archive_path,
                } => {
                    if let Some(works) = self.work_matching_tag.as_mut() {
                        if let Some(work) = works.get_mut(id) {
                            let preview_path = self.data_dir.join(preview_path);
                            let screen_path = self.data_dir.join(screen_path);
                            let archive_path = archive_path.as_ref().map(|a| self.data_dir.join(a));
                            work.set_paths(preview_path, screen_path, archive_path);
                            self.reproject_work();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn tag_selection(&self) -> &TagSet {
        &self.tag_selection
    }

    pub fn set_tag_selection(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        tag_set: TagSet,
        db: &DbReadHandle,
    ) {
        if &tag_set != self.tag_selection() {
            self.tag_selection = tag_set;
            self.tags_changed(tags, db);
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    fn tags_changed(&mut self, tags: Option<&HashMap<TagId, DbTag>>, db: &DbReadHandle) {
        if let Some(tag_id) = self.tag_selection.get_best_refresh(tags) {
            self.work_matching_tag = None;
            self.work_filtered = Vec::new();
            self.selected = None;
            db.get_works_for_tag(tag_id);
        } else if self.tag_selection.is_empty() {
            self.work_matching_tag = None;
            self.work_filtered = Vec::new();
            self.selected = None;
            db.get_favorite_works();
        }

        if self.tag_selection.reset_changed() {
            self.reproject_work();
        }
    }

    fn reproject_work(&mut self) {
        if let Some(works) = self.work_matching_tag.as_ref() {
            self.work_filtered = works
                .values()
                .filter(|work| work.screen_path().is_some())
                .filter(|work| {
                    (self.showing == WorkVisibility::All && !work.hidden())
                        || (self.showing == WorkVisibility::Favorites && work.favorite())
                        || (self.showing == WorkVisibility::Hidden && work.hidden())
                })
                .filter(|work| self.tag_selection.matches(work))
                .sorted_by(|a, b| {
                    let ord = match self.order.column {
                        WorkSortCol::Date => a.date().cmp(b.date()),
                    };
                    match self.order.order {
                        OrderDir::Asc => ord,
                        OrderDir::Desc => ord.reverse(),
                    }
                })
                .map(|work| work.id())
                .collect();
            if let Some(selected) = self.selected {
                if self.work_filtered.is_empty() {
                    self.selected = None;
                } else if selected >= self.work_filtered.len() {
                    self.selected = Some(self.work_filtered.len() - 1);
                }
            }
            info!(
                "Showing {} of {} matching works",
                self.work_filtered.len(),
                works.len()
            );
        } else {
            self.work_filtered = Vec::new();
        }
    }

    fn check_key_binds(&mut self, db_sync: &DbSyncHandle, n_wide: usize, ui: &egui::Ui) {
        const KEYS: [Key; 13] = [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::N,
            Key::P,
            Key::W,
            Key::A,
            Key::S,
            Key::D,
            Key::F6,
            Key::F7,
            Key::Delete,
        ];
        let mut pressed = HashSet::new();
        ui.ctx().input_mut(|input| {
            for key in &KEYS {
                if input.consume_key(Modifiers::NONE, *key) {
                    pressed.insert(*key);
                }
            }
        });

        // Some keys work the same in any mode
        let pressed_up = pressed.contains(&Key::ArrowUp) || pressed.contains(&Key::W);
        let pressed_down = pressed.contains(&Key::ArrowDown) || pressed.contains(&Key::S);
        let pressed_left = pressed.contains(&Key::ArrowLeft)
            || pressed.contains(&Key::P)
            || pressed.contains(&Key::A);
        let pressed_right = pressed.contains(&Key::ArrowRight)
            || pressed.contains(&Key::N)
            || pressed.contains(&Key::D);
        if let Some(selected) = self.selected {
            if pressed_left {
                self.selected = Some(selected.wrapping_sub(1).min(self.work_filtered.len() - 1));
            }
            if pressed_right {
                self.selected = Some(selected.saturating_add(1) % self.work_filtered.len());
            }
            if pressed_up {
                self.selected = Some(selected.saturating_sub(n_wide));
            }
            if pressed_down {
                self.selected = Some(
                    selected
                        .saturating_add(n_wide)
                        .min(self.work_filtered.len() - 1),
                );
            }
            if let Some(work) = self.get_selected_work_mut() {
                if pressed.contains(&Key::F6) {
                    db_sync
                        .set_work_favorite(work.id(), true)
                        .expect("set favorite");
                    work.set_favorite(true);
                } else if pressed.contains(&Key::F7) {
                    db_sync
                        .set_work_favorite(work.id(), false)
                        .expect("set favorite");
                    work.set_favorite(false);
                } else if pressed.contains(&Key::Delete) {
                    db_sync
                        .set_work_hidden(work.id(), !work.hidden())
                        .expect("set favorite");
                    work.set_hidden(!work.hidden());
                    self.reproject_work();
                }
            }
        }
    }

    pub fn get_selected_work(&self) -> Option<&DbWork> {
        self.work_matching_tag.as_ref().and_then(|m| {
            self.selected
                .map(|offset| &self.work_filtered[offset])
                .and_then(|id| m.get(id))
        })
    }

    pub fn get_selected_work_mut(&mut self) -> Option<&mut DbWork> {
        self.work_matching_tag.as_mut().and_then(|m| {
            self.selected
                .and_then(|offset| self.work_filtered.get_mut(offset))
                .and_then(|id| m.get_mut(id))
        })
    }

    pub fn info_ui(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db: &DbReadHandle,
        ui: &mut egui::Ui,
    ) {
        let Some(works) = self.work_matching_tag.as_ref() else {
            ui.spinner();
            return;
        };
        let Some(offset) = self.selected else {
            return;
        };
        let Some(work_id) = self.work_filtered.get(offset) else {
            return;
        };

        let work = &works[work_id];
        egui::Grid::new("work_info_grid").show(ui, |ui| {
            ui.label("Offset");
            ui.label(format!("{offset} of {}", self.work_filtered.len()));
            ui.end_row();

            ui.label("Name");
            ui.label(work.name());
            ui.end_row();

            ui.label("Date");
            ui.label(format!("{}", work.date()));
            ui.end_row();

            ui.label("Preview");
            ui.label(work.preview_url());
            ui.end_row();

            ui.label("Screen");
            ui.label(work.screen_url());
            ui.end_row();

            ui.label("Archive");
            ui.label(work.archive_url().unwrap_or(""));
            ui.end_row();

            if let Some(path) = work.screen_path() {
                let path = self.data_dir.join(path);
                if ui.button("Path 📋").clicked() {
                    ui.ctx().copy_text(path.display().to_string());
                }
                ui.label(path.display().to_string());
                ui.end_row();
            }
        });
        let mut changed = false;
        if let Some(tags) = tags {
            ui.label(" ");
            ui.heading("Tags");
            ui.separator();
            for tag_id in work.tags() {
                if let Some(tag) = tags.get(&tag_id)
                    && self.tag_selection.tag_row_ui(tag, ui)
                {
                    changed = true;
                }
            }
        }
        if changed {
            self.tags_changed(tags, db);
        }
    }

    pub fn gallery_ui(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db: &DbReadHandle,
        db_sync: &DbSyncHandle,
        perf: &mut PerfTrack,
        ui: &mut egui::Ui,
    ) {
        ui.horizontal(|ui| {
            if let Some(tags) = tags
                && self.tag_selection.ui(tags, ui)
            {
                self.tags_changed(Some(tags), db);
            }
            ui.label(format!("({})", self.work_filtered.len()));
            ui.add(
                egui::Slider::new(&mut self.thumb_size, 128f32..=512f32)
                    .text("Thumbnail Size")
                    .step_by(1.)
                    .fixed_decimals(0)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 })
                    .show_value(true)
                    .suffix("px"),
            );
            if self.showing.ui(ui) {
                self.reproject_work();
            }
        });
        if self.work_matching_tag.is_none() {
            ui.spinner();
            return;
        }

        let size = self.thumb_size;
        let width = ui.available_width();
        let n_wide = (width / size).floor().max(1.) as usize;
        let n_rows = self.work_filtered.len().div_ceil(n_wide);

        self.check_key_binds(db_sync, n_wide, ui);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, size, n_rows, |ui, rows| {
                // Overfetch by 1x our current visible area in both directions so we can
                // usually scroll in either direction without pause or loading spinners.
                //
                //  All works (ideal case shown; the actual slice may go before or after)
                //  |--------<  [  ]  >--|
                //              [  ] <- visible slice
                //           |        | <- query slice
                //           |--[  ]--| <- works slice
                //
                let visible_start = rows.start * n_wide;
                let visible_end = (rows.end * n_wide).min(self.work_filtered.len());
                let visible_slice = visible_start..visible_end;
                let win = visible_slice.len().max(10);
                let query_start = visible_slice.start.saturating_sub(win);
                let query_end = visible_slice
                    .end
                    .saturating_add(win)
                    .min(self.work_filtered.len());
                let query_slice = query_start..query_end;

                // Pre-scan the works slice to ask to pre-load all the images that
                // are in our query window (Note: this extends outside the visible area
                // to make scrolling faster).
                let cache_start = Instant::now();
                for work_offset in query_slice {
                    self.ensure_work_cached(ui.ctx(), work_offset);
                }
                self.flush_works_lru(ui.ctx());
                perf.sample("Cache Images", cache_start.elapsed());

                let sel_color = ui.style().visuals.selection.bg_fill;
                ui.style_mut().spacing.item_spacing = Vec2::ZERO;

                let draw_start = Instant::now();
                for row_work_offsets in &visible_slice.chunks(n_wide) {
                    ui.horizontal(|ui| {
                        for work_offset in row_work_offsets {
                            let work = &self
                                .work_matching_tag
                                .as_ref()
                                .expect("no work after check")[&self.work_filtered[work_offset]];

                            // Selection uses the selection color for the background
                            // let is_selected = self.state.selected_work.is_selected(work.id());
                            let is_selected = self.selected == Some(work_offset);

                            let img = self
                                .get_best_image(work, WorkSize::Preview)
                                .alt_text(work.name())
                                .show_loading_spinner(true)
                                .maintain_aspect_ratio(true);

                            let mut pad = 0.;
                            let mut inner_margin = Margin::ZERO;
                            if let Some(loaded_size) =
                                img.load_and_calc_size(ui, Vec2::new(size, size))
                            {
                                // Wide things are already centered for some reason,
                                // so we only need to care about tall images
                                if loaded_size.y > loaded_size.x {
                                    pad = (size - loaded_size.x) / 2.;
                                    inner_margin.left = pad as i8;
                                }
                            }

                            let btn = egui::ImageButton::new(img)
                                .frame(false)
                                .selected(is_selected)
                                .sense(Sense::click());

                            let mut frm = egui::Frame::default()
                                .outer_margin(Margin::ZERO)
                                .inner_margin(inner_margin);
                            if is_selected {
                                frm = frm.fill(sel_color);
                            }

                            let rsz = egui::Resize::default()
                                .min_size(Vec2::new(size - pad, size))
                                .max_size(Vec2::new(size - pad, size))
                                .default_size(Vec2::new(size - pad, size))
                                .resizable([false, false]);

                            frm.show(ui, |ui| {
                                rsz.show(ui, |ui| {
                                    let resp = ui.add(btn);
                                    if is_selected {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.clicked() {
                                        self.selected = Some(work_offset);
                                    }
                                });
                            });
                        }
                    });
                }
                perf.sample("Draw Works", draw_start.elapsed());
            });
    }

    pub fn slideshow_ui(&mut self, db_sync: &DbSyncHandle, ctx: &egui::Context) {
        let work_offset = self
            .selected
            .expect("entered slideshow without a selection");
        egui::CentralPanel::default().show(ctx, |ui| {
            // TODO: zoom and pan
            // See https://github.com/emilk/egui/blob/0f6310c598b5be92f339c9275a00d5decd838c1b/examples/custom_plot_manipulation/src/main.rs
            // for an example of how to do zoom and pan on a paint-like thing.

            let size = self.thumb_size;
            let width = ui.available_width();
            let n_wide = (width / size).floor().max(1.) as usize;
            self.check_key_binds(db_sync, n_wide, ui);

            self.ensure_work_cached(ui.ctx(), work_offset);
            for offset in work_offset.saturating_sub(10)
                ..work_offset.saturating_add(10).min(self.work_filtered.len())
            {
                self.ensure_work_cached(ui.ctx(), offset);
            }
            self.flush_works_lru(ui.ctx());

            // let work = &self.work_matching_tag[self.work_filtered[work_offset]];
            if let Some(works) = self.work_matching_tag.as_ref() {
                if let Some(work) = works.get(&self.work_filtered[work_offset]) {
                    let img = self
                        .get_best_image(work, WorkSize::Screen)
                        .show_loading_spinner(false)
                        .maintain_aspect_ratio(true);

                    let avail = ui.available_size();
                    if let Some(size) = img.load_and_calc_size(ui, avail) {
                        let (mut left, mut right, mut top, mut bottom) = (0., avail.x, 0., avail.y);
                        if avail.y > size.y {
                            top = (avail.y - size.y) / 2.;
                            bottom = avail.y - top;
                        }
                        if avail.x > size.x {
                            left = (avail.x - size.x) / 2.;
                            right = avail.x - left;
                        }
                        img.paint_at(ui, Rect::from_x_y_ranges(left..=right, top..=bottom));
                    }
                    ui.horizontal(|ui| {
                        ui.label(format!("{work_offset} of {}", self.work_filtered.len()));
                        if work.favorite() {
                            ui.label("✨");
                        }
                    });
                }
            }
        });
    }

    fn get_best_image<'b>(&self, work: &'b DbWork, req_sz: WorkSize) -> egui::Image<'b> {
        if matches!(req_sz, WorkSize::Screen)
            && let Some(screen_path) = work.screen_path()
        {
            let screen_uri = format!("file://{}", self.data_dir.join(screen_path).display());
            if self.works_lru.contains(&screen_uri) {
                return egui::Image::new(screen_uri);
            }
            // Note: Fall through to try to load the preview image
        }

        if let Some(preview_path) = work.preview_path() {
            let preview_uri = format!("file://{}", self.data_dir.join(preview_path).display());
            // println!("Would show: {preview_uri}: {}", self.state.works_lru.contains(&preview_uri));
            if self.works_lru.contains(&preview_uri) {
                return egui::Image::new(preview_uri);
            }
            // Note: fall through to load a fallback image
        }

        egui::Image::new(include_image!("../../assets/loading-preview.png"))
    }

    fn ensure_work_cached(
        &mut self,
        ctx: &egui::Context,
        work_offset: usize, /* work: &DbWork*/
    ) {
        // If we restore from an exit in slideshow mode and haven't loaded yet.
        if self.work_matching_tag.is_none() {
            return;
        }

        // Limit number of times we call try_load_image per frame to prevent pauses
        if self.per_frame_work_upload_count > Self::MAX_PER_FRAME_UPLOADS {
            return;
        }

        // Adjust the size hint to be one power-of-two larger than whatever
        // our current thumbnail size is set to. This will cause us to reload
        // images as we scale, keeping the thumbnails looking okay.
        let size = (self.thumb_size.round() as u32).next_power_of_two();
        let size_hint = SizeHint::Size {
            width: size,
            height: size,
            maintain_aspect_ratio: true,
        };
        if let Some(screen_path) = self
            .work_matching_tag
            .as_ref()
            .expect("no work after check")[&self.work_filtered[work_offset]]
            .screen_path()
        {
            let screen_uri = format!("file://{}", self.data_dir.join(screen_path).display());
            if !self.works_lru.contains(&screen_uri) {
                ctx.try_load_image(&screen_uri, size_hint).ok();
                self.per_frame_work_upload_count += 1;
                self.works_lru.get_or_insert(screen_uri, || 0);
            }
        }
        if let Some(preview_path) = self
            .work_matching_tag
            .as_ref()
            .expect("no work after check")[&self.work_filtered[work_offset]]
            .preview_path()
        {
            let preview_uri = format!("file://{}", self.data_dir.join(preview_path).display());
            if !self.works_lru.contains(&preview_uri) {
                ctx.try_load_image(&preview_uri, size_hint).ok();
                self.per_frame_work_upload_count += 1;
                self.works_lru.get_or_insert(preview_uri.clone(), || 0);
            }
        }
    }

    fn flush_works_lru(&mut self, ctx: &egui::Context) {
        self.per_frame_work_upload_count = 0;
        while self.works_lru.len() > Self::LRU_CACHE_SIZE {
            if let Some((uri, _)) = self.works_lru.pop_lru() {
                ctx.forget_image(&uri);
            }
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_next_power_of_two() {
        assert_eq!((127.5f32.round() as u32).next_power_of_two(), 128);
        assert_eq!((127.4f32.round() as u32).next_power_of_two(), 128);
        assert_eq!((128.1f32.round() as u32).next_power_of_two(), 128);
        assert_eq!((128.6f32.round() as u32).next_power_of_two(), 256);

        // let foo: u32 = self.thumb_size as u32;
        // foo.next_power_of_two();
    }
}
