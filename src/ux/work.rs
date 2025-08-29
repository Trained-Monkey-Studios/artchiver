use crate::{
    db::{
        models::{
            tag::{DbTag, TagId},
            work::{DbWork, WorkId},
        },
        {model::OrderDir, reader::DbReadHandle, writer::DbWriteHandle},
    },
    plugin::{host::PluginHost, thumbnail::is_image},
    shared::{
        performance::PerfTrack,
        tag::{TagRefresh, TagSet},
        update::DataUpdate,
    },
};
use anyhow::Result;
use egui::{Key, Margin, Modifiers, PointerButton, Rect, Sense, SizeHint, Vec2, include_image};
use egui_video::{AudioDevice, Player};
use itertools::Itertools as _;
use log::{info, trace};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum DisplayKind<'a> {
    Image(egui::Image<'a>),
    MediaPlayer,
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
        let labels = ["Date"];
        egui::ComboBox::new("tag_order_column", "")
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
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let prior = *self;
        self.column.ui(ui);
        self.order.ui("tags", ui);
        *self != prior
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
        egui::ComboBox::new("work_visibility", "")
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

#[derive(Clone, Debug)]
pub struct ZoomPan {
    zoom: f32,
    pan: Vec2,
}

impl Default for ZoomPan {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: Vec2::ZERO,
        }
    }
}

impl ZoomPan {
    pub fn zoom_in(&mut self, pos: Vec2) {
        self.zoom *= 1.1;

        // Note: adjust the pan when we change zoom levels. We want to maintain the spot under
        // `pos` having the same visual position after the zoom. e.g. If `pos` is the top left
        // corner, then we want to shift nowhere. If pos is the bottom right corner, on the other
        // hand, we need to shift by the full delta in the shown area after the zoom.
        let prior_edge_to_pos = pos - self.pan;
        let next_edge_to_pos = prior_edge_to_pos * 1.1;
        self.pan = pos - next_edge_to_pos;
    }

    pub fn zoom_out(&mut self, pos: Vec2) {
        let prior_zoom = self.zoom;
        self.zoom /= 1.1;
        self.zoom = self.zoom.max(1.0);

        // zoom' = zoom / x;
        // zoom = zoom' * x;
        // x = zoom / zoom';
        let effective_zoom = prior_zoom / self.zoom;

        let prior_edge_to_pos = pos - self.pan;
        let next_edge_to_pos = prior_edge_to_pos / effective_zoom;
        self.pan = pos - next_edge_to_pos;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.pan += delta;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ScrollRequestKind {
    // No movement requested
    None,
    // Selection was moved via interaction in the UX. Move the viewport the minimum
    // amount required to keep the newly selected item in view.
    Movement,
    // The user just left the slideshow view. Move the viewport to the currently selected item
    // if it is not already in view. Center the item, since the move may have been large.
    LeaveSlideshow,
}

fn init_audio_device() -> AudioDevice {
    AudioDevice::new().expect("Failed to create audio output")
}

/// Work caching strategy:
///
/// Works are unbounded, but plan to scale to O(10-100M) works. This is too much for us to just
/// read everything, so we have to read blocks of the potentially visible set, based on what
/// is active in the tag set. Changing tag sets triggers a re-read of the database, but it would
/// be nice if we could just show everything if there are no tags.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct UxWork {
    // Offset into work_filtered.
    selected: Option<usize>,
    thumb_size: f32,

    // Filter state for the works gallery
    tag_selection: TagSet,
    order: WorkOrder,

    #[serde(skip)]
    scroll_to_selected: ScrollRequestKind,

    #[serde(skip)]
    last_mouse_motion: Instant,

    #[serde(skip)]
    showing: WorkVisibility,

    #[serde(skip)]
    slide_xform: ZoomPan,

    #[serde(skip)]
    work_reproject_timer: Option<Instant>,

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

    #[serde(skip, default = "init_audio_device")]
    audio_device: AudioDevice,

    #[serde(skip, default)]
    video_player: Option<Player>,
}

impl Default for UxWork {
    fn default() -> Self {
        Self {
            selected: None,
            thumb_size: 128.,
            scroll_to_selected: ScrollRequestKind::None,
            tag_selection: TagSet::default(),
            order: WorkOrder::default(),
            last_mouse_motion: Instant::now(),
            showing: WorkVisibility::default(),
            slide_xform: ZoomPan::default(),
            work_reproject_timer: None,
            per_frame_work_upload_count: 0,
            work_matching_tag: None,
            work_filtered: Vec::new(),
            data_dir: PathBuf::new(),
            works_lru: LruCache::unbounded(),
            audio_device: init_audio_device(),
            video_player: None,
        }
    }
}

impl UxWork {
    const LRU_CACHE_SIZE: usize = 500;
    const MAX_PER_FRAME_UPLOADS: usize = 3;

    pub fn startup(&mut self, data_dir: &Path, db: &DbReadHandle) {
        trace!("Starting up work UX");

        self.data_dir = data_dir.to_owned();

        // FIXME: this is going to fetch the wrong thing. We want the smallest tag, as selected elsewhere.
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
        // Note: we only care about reprojection cost incurred _not_ by the user: e.g. through
        //       messages (e.g. database changes). We always need to record the changes, but we
        //       don't have to immediately show the changes if it's going to lag the UX.
        if let Some(start) = self.work_reproject_timer
            && start.elapsed() > Duration::from_secs(4)
        {
            self.work_reproject_timer = None;
            self.reproject_work(tags);
        }

        for update in updates {
            match update {
                DataUpdate::ListWorksChunk { tag_id, works } => {
                    if *tag_id == self.tag_selection.last_fetched() {
                        trace!("Received {} works for tag {tag_id:?}", works.len());
                        if let Some(local) = self.work_matching_tag.as_mut() {
                            local.extend(works.iter().map(|(id, work)| (*id, work.to_owned())));
                        } else {
                            self.work_matching_tag = Some(works.to_owned());
                        }
                        self.reproject_work(tags);
                    } else {
                        trace!(
                            "Ignoring works for tag {tag_id:?} (expected {:?})",
                            self.tag_selection.last_fetched()
                        );
                    }
                }
                DataUpdate::InitialTags(_) => {
                    self.tag_selection.force_refresh();
                }
                DataUpdate::WorksWereUpdatedForTag { for_tag } => {
                    if self.tag_selection.enabled().any(|id| {
                        tags.and_then(|tags| tags.get(&id)).map(|tag| tag.name())
                            == Some(for_tag.as_str())
                    }) {
                        self.tag_selection.force_refresh();
                    }
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
                            if self.work_reproject_timer.is_none() {
                                self.work_reproject_timer = Some(Instant::now());
                            }
                        }
                    }
                }
                DataUpdate::TagHiddenStatusChanged { .. } => {
                    self.reproject_work(tags);
                }
                _ => {}
            }
        }

        // Check tag freshness
        self.ensure_works_up_to_date_with_tag_selection(tags, db);
    }

    pub fn tag_selection(&self) -> &TagSet {
        &self.tag_selection
    }

    pub fn tag_selection_mut(&mut self) -> &mut TagSet {
        &mut self.tag_selection
    }

    pub fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    pub fn set_selected(&mut self, selected: usize) {
        self.selected = Some(selected);
        self.video_player = None;
        self.slide_xform = ZoomPan::default();
    }

    pub fn clear_selected(&mut self) {
        self.selected = None;
        self.video_player = None;
        self.slide_xform = ZoomPan::default();
    }

    pub fn on_leave_slideshow(&mut self) {
        self.scroll_to_selected = ScrollRequestKind::LeaveSlideshow;
        self.video_player = None;
    }

    fn ensure_works_up_to_date_with_tag_selection(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db: &DbReadHandle,
    ) {
        match self.tag_selection.get_best_refresh(tags) {
            TagRefresh::NoneNeeded => {}
            TagRefresh::NeedReproject => {
                self.reproject_work(tags);
            }
            TagRefresh::NeedRefresh(tag_id) => {
                self.work_matching_tag = None;
                self.work_filtered = Vec::new();
                self.clear_selected();
                db.get_works_for_tag(tag_id);
            }
            TagRefresh::Favorites => {
                self.work_matching_tag = None;
                self.work_filtered = Vec::new();
                self.clear_selected();
                db.get_favorite_works();
            }
        }
    }

    fn reproject_work(&mut self, tags: Option<&HashMap<TagId, DbTag>>) {
        if let Some(works) = self.work_matching_tag.as_ref() {
            let selected = self.get_selected_work().map(|w| w.id());
            self.work_filtered = works
                .values()
                // Only show works that we can actually show.
                .filter(|work| work.screen_path().is_some())
                // Filter out hidden or favorite works if we're not showing them.
                .filter(|work| {
                    (self.showing == WorkVisibility::All && !work.hidden())
                        || (self.showing == WorkVisibility::Favorites && work.favorite())
                        || (self.showing == WorkVisibility::Hidden && work.hidden())
                })
                // Only show works that match the current tag selection.
                .filter(|work| self.tag_selection.matches(work))
                // Filter our any works with tags that have been hidden.
                .filter(|work| {
                    if let Some(tags) = tags {
                        for tag_id in work.tags() {
                            if let Some(tag) = tags.get(&tag_id) {
                                if tag.hidden() {
                                    return false;
                                }
                            }
                        }
                    }
                    true
                })
                .sorted_by(|a, b| {
                    let ord = match self.order.column {
                        WorkSortCol::Date => match a.date().cmp(b.date()) {
                            Ordering::Equal => a.id().cmp(&b.id()),
                            v => v,
                        },
                    };
                    match self.order.order {
                        OrderDir::Asc => ord,
                        OrderDir::Desc => ord.reverse(),
                    }
                })
                .map(|work| work.id())
                .collect();
            info!(
                "Showing {} of {} matching works",
                self.work_filtered.len(),
                works.len()
            );
            // The position of the selected work may have changed in our newly filtered list.
            // Re-look-up the position of the selected id. If the selected id is no longer in
            // the filtered list, the selection will become None via the and_then.
            self.selected =
                selected.and_then(|id| self.work_filtered.iter().position(|i| *i == id));
        } else {
            self.work_filtered = Vec::new();
        }
    }

    fn get_pressed_keys(ui: &egui::Ui, keys: &[Key]) -> HashSet<Key> {
        let mut pressed = HashSet::new();
        ui.ctx().input_mut(|input| {
            for key in keys {
                if input.consume_key(Modifiers::NONE, *key) {
                    pressed.insert(*key);
                }
            }
        });
        pressed
    }

    fn check_common_key_binds(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db_write: &DbWriteHandle,
        n_wide: usize,
        ui: &egui::Ui,
    ) {
        let pressed = Self::get_pressed_keys(
            ui,
            &[
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
            ],
        );

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
                self.set_selected(selected.wrapping_sub(1).min(self.work_filtered.len() - 1));
                self.scroll_to_selected = ScrollRequestKind::Movement;
            }
            if pressed_right {
                self.set_selected(selected.saturating_add(1) % self.work_filtered.len());
                self.scroll_to_selected = ScrollRequestKind::Movement;
            }
            if pressed_up {
                self.set_selected(selected.saturating_sub(n_wide));
                self.scroll_to_selected = ScrollRequestKind::Movement;
            }
            if pressed_down {
                self.set_selected(
                    selected
                        .saturating_add(n_wide)
                        .min(self.work_filtered.len() - 1),
                );
                self.scroll_to_selected = ScrollRequestKind::Movement;
            }
            let selected = self.selected;
            if let Some(work) = self.get_selected_work_mut() {
                if pressed.contains(&Key::F6) {
                    db_write
                        .set_work_favorite(work.id(), true)
                        .expect("set favorite");
                    work.set_favorite(true);
                } else if pressed.contains(&Key::F7) {
                    db_write
                        .set_work_favorite(work.id(), false)
                        .expect("set favorite");
                    work.set_favorite(false);
                } else if pressed.contains(&Key::Delete) {
                    db_write
                        .set_work_hidden(work.id(), !work.hidden())
                        .expect("set favorite");
                    work.set_hidden(!work.hidden());
                    self.reproject_work(tags);
                    self.selected = selected;
                }
            }
        }
    }

    fn check_slideshow_key_binds(&mut self, ui: &egui::Ui) {
        let pressed = Self::get_pressed_keys(ui, &[Key::Equals, Key::Plus, Key::Minus, Key::Num0]);
        if pressed.contains(&Key::Plus) || pressed.contains(&Key::Equals) {
            self.slide_xform.zoom_in(ui.available_size() / 2.);
        }
        if pressed.contains(&Key::Minus) {
            self.slide_xform.zoom_out(ui.available_size() / 2.);
        }
        if pressed.contains(&Key::Num0) {
            self.slide_xform.reset();
        }

        ui.ctx().input_mut(|input| {
            if input.pointer.button_down(PointerButton::Primary) {
                if let Some(motion) = input.pointer.motion() {
                    self.slide_xform.pan(motion);
                }
            }
            if input.raw_scroll_delta.y > 0. {
                self.slide_xform
                    .zoom_in(input.pointer.hover_pos().unwrap_or_default().to_vec2());
            } else if input.raw_scroll_delta.y < 0. {
                self.slide_xform
                    .zoom_out(input.pointer.hover_pos().unwrap_or_default().to_vec2());
            }
        });
    }

    pub fn get_selected_work(&self) -> Option<&DbWork> {
        self.work_matching_tag.as_ref().and_then(|m| {
            self.selected
                .and_then(|offset| self.work_filtered.get(offset))
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
        db_write: &DbWriteHandle,
        host: &mut PluginHost,
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
        if let Some(tags) = tags {
            ui.label(" ");
            ui.horizontal(|ui| {
                ui.heading("Tags");
                if ui.button("✔ Select All").clicked() {
                    self.tag_selection.clear();
                    for tag in work.tags().filter_map(|tag_id| tags.get(&tag_id)) {
                        self.tag_selection.enable(tag);
                    }
                }
            });
            ui.separator();
            work.tags()
                .filter_map(|tag_id| tags.get(&tag_id))
                .sorted_by_key(|tag| tag.name())
                .for_each(|tag| {
                    self.tag_selection.tag_row_ui(tag, host, db_write, ui);
                });
        }
    }

    pub fn gallery_ui(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db_write: &DbWriteHandle,
        perf: &mut PerfTrack,
        ui: &mut egui::Ui,
    ) {
        ui.horizontal_wrapped(|ui| {
            if let Some(tags) = tags {
                self.tag_selection.ui(tags, ui);
            }
            ui.label(format!("({})", self.work_filtered.len()));
        });
        ui.horizontal(|ui| {
            ui.label("Sort");
            if self.order.ui(ui) {
                self.reproject_work(tags);
            }

            ui.separator();

            ui.label("Showing");
            if self.showing.ui(ui) {
                self.reproject_work(tags);
            }

            ui.separator();

            ui.label("Size");
            ui.add(
                egui::Slider::new(&mut self.thumb_size, 128f32..=512f32)
                    .step_by(1.)
                    .fixed_decimals(0)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 })
                    .show_value(true)
                    .suffix("px"),
            );
        });
        if self.work_matching_tag.is_none() {
            ui.spinner();
            return;
        }

        let size = self.thumb_size;
        let width = ui.available_width();
        let n_wide = (width / size).floor().max(1.) as usize;
        let n_rows = self.work_filtered.len().div_ceil(n_wide);

        self.check_common_key_binds(tags, db_write, n_wide, ui);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, size, n_rows, |ui, rows| {
                // We may have advanced past the area covered by `rows`, so we might not
                // do a draw call on the selected item, which means that we wouldn't ever
                // call a scroll-to on the response, so the scroll would just not happen.
                // Instead, we construct a virtual rect in the scroll area to scroll to.
                //
                // This is tricky. The viewport max-rect (in this case cursor, since we're
                // doing this first) is the offset of the current viewport relative
                // to rows. So if we're at the top, it will be the offset from the top of
                // the window to the top of the scroll area. As we scroll down, this lowers
                // to zero when the top of the first drawn item is at the top of the window
                // (hidden behind the menubar and whatnot). As we scroll further, this goes
                // negative as the logical top of the first row in the viewport goes above
                // the top of the window. Once the row is fully out of view, the cursor
                // resets to the next row, with the offset returning to the distance from
                // the top of window, to the top of the scroll area.
                //
                // So it seems like we need to have the rect relative to the top of the
                // `window`.
                //
                //  - - - - - -  top of scroll
                //  |-|-|-|-|-|
                //  -----------  top of window (0)
                //  |  menus  |
                //  -----------  cursor.top()
                //  -----------  top of viewport
                //  |-|-|-|-|-|
                //  |-|-|||-|-|  selected
                //  |-|-|-|-|-|
                //  -----------  bottom of window
                if let Some(selected) = self.selected
                    && self.scroll_to_selected != ScrollRequestKind::None
                    && !(rows.start..rows.end.saturating_sub(1)).contains(&(selected / n_wide))
                {
                    let selected_row = selected / n_wide;
                    let scroll_to_selected = selected_row as f32 * size;
                    let scroll_to_cursor = rows.start as f32 * size;
                    let cursor_to_selected = scroll_to_selected - scroll_to_cursor;
                    let selected_to_window = ui.cursor().top() + cursor_to_selected;
                    let y = selected_to_window;
                    let rect = Rect::from_x_y_ranges(0f32..=10f32, y..=y + size);
                    match self.scroll_to_selected {
                        ScrollRequestKind::LeaveSlideshow => {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                        ScrollRequestKind::Movement => {
                            ui.scroll_to_rect(rect, None);
                        }
                        ScrollRequestKind::None => {}
                    }
                    self.scroll_to_selected = ScrollRequestKind::None;
                }

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
                            let is_selected = self.selected == Some(work_offset);

                            let img = self
                                .get_preview_image(work)
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
                                    if resp.clicked() {
                                        self.selected = Some(work_offset);
                                        self.slide_xform = ZoomPan::default();
                                    }
                                });
                            });
                        }
                    });
                }
                perf.sample("Draw Works", draw_start.elapsed());
            });
    }

    pub fn slideshow_ui(
        &mut self,
        tags: Option<&HashMap<TagId, DbTag>>,
        db_write: &DbWriteHandle,
        ctx: &egui::Context,
    ) {
        let work_offset = self
            .selected
            .expect("entered slideshow without a selection");
        egui::CentralPanel::default().show(ctx, |ui| {
            let size = self.thumb_size;
            let width = ui.available_width();
            let n_wide = (width / size).floor().max(1.) as usize;
            self.check_common_key_binds(tags, db_write, n_wide, ui);
            self.check_slideshow_key_binds(ui);

            self.ensure_work_cached(ui.ctx(), work_offset);
            for offset in work_offset.saturating_sub(10)
                ..work_offset
                    .saturating_add(10)
                    .min(self.work_filtered.len().saturating_sub(1))
            {
                self.ensure_work_cached(ui.ctx(), offset);
            }
            self.flush_works_lru(ui.ctx());

            // if let Some(work) = self.get_selected_work() {
            match self.get_screen_image(ui.ctx()) {
                Err(err) => {
                    println!("Error: {}", err.backtrace());
                    ui.label(format!("Error loading media: {err:#?}"));
                }
                Ok(DisplayKind::Image(img)) => {
                    let img = img.show_loading_spinner(false).maintain_aspect_ratio(true);

                    let avail = ui.available_size() * self.slide_xform.zoom;
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
                        let rect = Rect::from_x_y_ranges(left..=right, top..=bottom)
                            .translate(self.slide_xform.pan);
                        img.paint_at(ui, rect);
                    }
                    self.draw_offset_label(ui, work_offset);
                }
                Ok(DisplayKind::MediaPlayer) => {
                    let (src, _elapsed) = if let Some(player) = self.video_player.as_ref() {
                        (player.size, player.elapsed_ms())
                    } else {
                        (ui.available_size(), 0)
                    };

                    // FIXME: store aside elapsed into a state that will get saved between runs
                    //        so that when we start up again we'll resume in a podcast where we
                    //        left off.

                    let dst = ui.available_size();
                    let scale = (dst.x / src.x).min(dst.y / src.y);
                    let pad = (dst - src * scale) / 2.;
                    if pad.x > 0. {
                        ui.horizontal(|ui| {
                            let v = Vec2::new(pad.x, dst.y);
                            egui::Resize::default()
                                .min_size(v)
                                .max_size(v)
                                .default_size(v)
                                .resizable([false, false])
                                .show(ui, |ui| {
                                    // FIXME: figure out how to overlay the work offset label
                                    ui.label("");
                                });
                            if let Some(player) = self.video_player.as_mut() {
                                player.ui(ui, src * scale);
                                // player.subtitle_streamer.as_ref().map(|ss| ss.lock())
                            }
                        });
                    } else {
                        ui.vertical(|ui| {
                            let v = Vec2::new(dst.x, pad.y);
                            egui::Resize::default()
                                .min_size(v)
                                .max_size(v)
                                .default_size(v)
                                .resizable([false, false])
                                .show(ui, |ui| {
                                    // self.draw_offset_label(ui, work_offset);
                                    ui.label(format!(
                                        "{work_offset} of {} {}",
                                        self.work_filtered.len(),
                                        self.get_selected_work()
                                            .map(|w| w.favorite_annotation())
                                            .unwrap_or_default()
                                    ));
                                });
                            if let Some(player) = self.video_player.as_mut() {
                                player.ui(ui, src * scale);
                            }
                        });
                    }
                }
            }
        });

        // Hide the mouse cursor on inactivity
        let mouse_is_moving = ctx.input_mut(|input| {
            input.raw_scroll_delta != Vec2::ZERO
                || input.pointer.button_down(PointerButton::Primary)
                || input.pointer.button_down(PointerButton::Secondary)
                || (input.pointer.motion().is_some() && input.pointer.motion() != Some(Vec2::ZERO))
        });
        if mouse_is_moving {
            self.last_mouse_motion = Instant::now();
            // Note: make sure we call through this path again after we might expire, otherwise
            //       the cursor won't get hidden, because we don't redraw if we're inactive
            ctx.request_repaint_after(Duration::from_secs(2));
        } else if self.last_mouse_motion.elapsed() < Duration::from_secs(2) {
            // Note: we may have gotten painted again after the above check but before we make it
            //       below, so request repaint again until we hit the 2 second timeout
            ctx.request_repaint_after(Duration::from_secs(2) - self.last_mouse_motion.elapsed());
        } else if self.last_mouse_motion.elapsed() >= Duration::from_millis(1900) {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }

    fn draw_offset_label(&self, ui: &mut egui::Ui, offset: usize) {
        ui.label(format!(
            "{offset} of {} {}",
            self.work_filtered.len(),
            self.get_selected_work()
                .map(|w| w.favorite_annotation())
                .unwrap_or_default()
        ));
    }

    fn get_preview_image<'b>(&self, work: &'b DbWork) -> egui::Image<'b> {
        if let Some(preview_path) = work.preview_path() {
            let preview_uri = format!("file://{}", self.data_dir.join(preview_path).display());
            // println!("Would show: {preview_uri}: {}", self.state.works_lru.contains(&preview_uri));
            if self.works_lru.contains(&preview_uri) {
                return egui::Image::new(preview_uri);
            }
        }
        egui::Image::new(include_image!("../../assets/loading-preview.png"))
    }

    fn get_screen_image<'b>(&mut self, ctx: &egui::Context) -> Result<DisplayKind<'b>> {
        if let Some(work) = self.get_selected_work()
            && let Some(screen_path) = work.screen_path()
        {
            let screen_path = self.data_dir.join(screen_path);
            let screen_path_str = screen_path.display().to_string();
            let screen_uri = format!("file://{screen_path_str}");
            if is_image(&screen_path) {
                if self.works_lru.contains(&screen_uri) {
                    return Ok(DisplayKind::Image(egui::Image::new(screen_uri)));
                }
            } else if self.video_player.is_none() {
                self.video_player = Some(
                    Player::new(ctx, &screen_path_str)?
                        .with_audio(&mut self.audio_device)?
                        .with_subtitles()?,
                );
                self.video_player.as_mut().expect("created").start();
                return Ok(DisplayKind::MediaPlayer);
            } else {
                return Ok(DisplayKind::MediaPlayer);
            }
        }

        // Note: Fall through to try to load the preview image so we have something to show.
        Ok(DisplayKind::Image(egui::Image::new(include_image!(
            "../../assets/loading-preview.png"
        ))))
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
            && is_image(screen_path)
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
            // Note: non-image previews will just show up as an error icon; the thumbnailing
            //       should already have happened out of line.
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
