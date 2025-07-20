use crate::{
    shared::{environment::Environment, performance::PerfTrack, progress::Progress, tag::TagSet},
    sync::{
        db::tag::TagOrder,
        plugin::{
            client::get_data_path_for_url,
            host::{PluginHandle, PluginHost},
        },
    },
};
use anyhow::Result;
use artchiver_sdk::Work;
use egui::{
    self, Key, Margin, Modifiers, Rect, Sense, SizeHint, TextWrapMode, Vec2, include_image,
};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use itertools::Itertools as _;
use log::Level;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

// Utility function to get an egui margin inset from the left.
fn indented(px: i8) -> Margin {
    let mut m = Margin::ZERO;
    m.left = px;
    m
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabMetadata {
    title: String,
}

impl TabMetadata {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum WorkSize {
    // Thumbnail,
    Preview,
    Screen,
    // Archive
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum WorkSelection {
    #[default]
    None,
    Work {
        offset: usize,
        // zoom: f32,
        // pan: (f32, f32),
    },
}

impl WorkSelection {
    pub fn new(offset: usize) -> Self {
        Self::Work { offset }
    }

    pub fn is_selected(&self, offset: usize) -> bool {
        match self {
            Self::None => false,
            Self::Work { offset: idx, .. } => *idx == offset,
        }
    }

    pub fn get_selected_offset(&self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Work { offset, .. } => Some(*offset),
        }
    }

    pub fn move_to_next(&mut self) {
        match self {
            Self::None => {}
            Self::Work { offset } => *offset = offset.wrapping_add(1),
        }
    }

    pub fn move_to_prev(&mut self) {
        match self {
            Self::None => {}
            Self::Work { offset } => *offset = offset.saturating_sub(1),
        }
    }

    pub fn has_selection(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UxState {
    mode: UxMode,
    show_preferences: bool,
    show_performance: bool,
    show_about: bool,
    tag_filter: String,
    tag_selection: TagSet,
    tag_source: Option<String>,
    tag_order: TagOrder,
    selected_work: WorkSelection,
    thumb_size: f32,

    #[serde(skip)]
    perf: PerfTrack,

    #[serde(skip, default = "LruCache::unbounded")]
    works_lru: LruCache<String, u32>,

    #[serde(skip)]
    per_frame_work_upload_count: usize,
}

impl Default for UxState {
    fn default() -> Self {
        Self {
            mode: UxMode::Browser,
            show_preferences: false,
            show_performance: false,
            show_about: false,
            tag_filter: String::new(),
            tag_selection: TagSet::default(),
            tag_source: None,
            tag_order: TagOrder::default(),
            thumb_size: 256.,
            perf: PerfTrack::default(),
            works_lru: LruCache::unbounded(),
            per_frame_work_upload_count: 0,
            selected_work: WorkSelection::None,
        }
    }
}

impl UxState {
    const LRU_CACHE_SIZE: usize = 1_000;
    const MAX_PER_FRAME_UPLOADS: usize = 3;
}

struct SyncViewer<'a> {
    sync: &'a mut PluginHost,
    state: &'a mut UxState,
    data_dir: &'a Path,
}

impl<'a> SyncViewer<'a> {
    fn wrap(sync: &'a mut PluginHost, state: &'a mut UxState, data_dir: &'a Path) -> Self {
        Self {
            sync,
            state,
            data_dir,
        }
    }

    fn show_plugin_details(ui: &mut egui::Ui, plugin: &mut PluginHandle) {
        egui::CollapsingHeader::new("Details")
            .id_salt(format!("details_section_{}", plugin.name()))
            .show(ui, |ui| -> Result<()> {
                ui.label(plugin.description());
                egui::Grid::new(format!("plugin_grid_{}", plugin.name()))
                    .num_columns(2)
                    .show(ui, |ui| -> Result<()> {
                        ui.label("Source");
                        ui.label(plugin.source().display().to_string());
                        ui.end_row();
                        ui.label("Version");
                        ui.label(plugin.version());
                        ui.end_row();
                        if let Some(meta) = plugin.metadata_mut() {
                            for (config_key, config_val) in meta.configurations_mut() {
                                ui.label(config_key);
                                ui.text_edit_singleline(config_val);
                                ui.end_row();
                            }
                            if !meta.configurations().is_empty() && ui.button("Update").clicked() {
                                plugin.apply_configuration()?;
                            }
                        }
                        Ok(())
                    })
                    .inner?;
                Ok(())
            });
    }

    fn show_plugin_tasks(ui: &mut egui::Ui, plugin: &mut PluginHandle) {
        egui::CollapsingHeader::new("Tasks")
            .id_salt(format!("tasks_section_{}", plugin.name()))
            .show(ui, |ui| {
                match plugin.active_task() {
                    Some(task) => {
                        ui.horizontal(|ui| {
                            ui.label(format!("Current Task: {task}"));
                            if !plugin.cancellation().is_cancelled() {
                                if ui.small_button("x Cancel").clicked() {
                                    plugin.cancellation().cancel();
                                }
                            } else {
                                ui.label("Cancelling...");
                            }
                        });
                    }
                    None => {
                        ui.label("Inactive");
                    }
                }
                ui.separator();
                let mut removed = None;
                for (i, task) in plugin.task_queue().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("x").on_hover_text("Cancel").clicked() {
                            removed = Some(i);
                        }
                        ui.label(format!("{i}: {task}"));
                    });
                }
                if let Some(index) = removed {
                    plugin.remove_queued_task(index);
                }
            });
    }

    fn show_plugin_logs(ui: &mut egui::Ui, plugin: &PluginHandle) {
        egui::CollapsingHeader::new("Logs")
            .id_salt(format!("logs_section_{}", plugin.name()))
            .show(ui, |ui| {
                for (level, message) in plugin.log_messages() {
                    let msg = egui::RichText::new(message);
                    let msg = match level {
                        Level::Error => msg.strong().color(egui::Color32::RED),
                        Level::Warn => msg.color(egui::Color32::YELLOW),
                        Level::Info => msg.color(egui::Color32::GREEN),
                        Level::Debug => msg.color(egui::Color32::LIGHT_BLUE),
                        Level::Trace => msg.color(egui::Color32::LIGHT_GRAY),
                    };
                    ui.add(egui::Label::new(msg).wrap_mode(TextWrapMode::Truncate));
                }
            });
    }

    fn show_galleries(&mut self, ui: &mut egui::Ui) -> Result<()> {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| -> Result<()> {
                let mut remove = None;
                for (i, error) in self.sync.errors().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("x").clicked() {
                            remove = Some(i);
                        }
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(error)
                                    .strong()
                                    .color(egui::Color32::RED),
                            )
                            .wrap_mode(TextWrapMode::Truncate),
                        );
                    });
                }
                if let Some(index) = remove {
                    self.sync.remove_error(index);
                }
                for plugin in self.sync.plugins_mut() {
                    ui.horizontal(|ui| {
                        ui.heading(plugin.name());
                        if ui.button("⟳ Tags").clicked() {
                            plugin.refresh_tags();
                        }
                        match plugin.progress() {
                            Progress::None => {}
                            Progress::Spinner => {
                                ui.spinner();
                            }
                            Progress::Percent { current, total } => {
                                ui.add(
                                    egui::ProgressBar::new(*current as f32 / *total as f32)
                                        .animate(true)
                                        .show_percentage(),
                                );
                            }
                        }
                    });
                    egui::Frame::new()
                        .inner_margin(indented(16))
                        .show(ui, |ui| -> Result<()> {
                            Self::show_plugin_details(ui, plugin);
                            Self::show_plugin_tasks(ui, plugin);
                            Self::show_plugin_logs(ui, plugin);
                            Ok(())
                        })
                        .inner?;
                }
                Ok(())
            })
            .inner?;
        Ok(())
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let tag_cnt = self
            .sync
            .pool_mut()
            .count_tags(&self.state.tag_filter, self.state.tag_source.as_deref())?;
        // Filter and view bar
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.state.tag_filter);
            if ui.button("x").clicked() {
                self.state.tag_filter.clear();
            }
            ui.label(format!("({tag_cnt})",));

            let mut selected = 0usize;
            let mut options = self.sync.plugins().map(|p| p.name()).collect::<Vec<_>>();
            options.insert(0, "All".to_owned());
            if let Some(source) = self.state.tag_source.as_deref() {
                if let Some((offset, _)) = options.iter().find_position(|v| v == &source) {
                    selected = offset;
                }
            }
            egui::ComboBox::new("tag_filter_sources", "Source:")
                .wrap_mode(TextWrapMode::Truncate)
                .show_index(ui, &mut selected, options.len(), |i| &options[i]);
            if options[selected] == "All" {
                self.state.tag_source = None;
            } else {
                self.state.tag_source = Some(options[selected].clone());
            }
        });
        // Sorting
        ui.horizontal(|ui| {
            self.state.tag_order.ui(ui);
        });

        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(
                ui,
                row_height,
                tag_cnt.try_into()?,
                |ui, row_range| -> Result<()> {
                    let tags = self.sync.pool_mut().list_tags(
                        row_range,
                        &self.state.tag_filter,
                        self.state.tag_source.as_deref(),
                        self.state.tag_order
                    )?;

                    egui::Grid::new("tag_grid")
                        .num_columns(1)
                        .spacing([0., 0.])
                        .min_col_width(0.)
                        .show(ui, |ui| {
                            for tag in tags {
                                let status = self.state.tag_selection.status(tag.name());
                                if ui
                                    .add(egui::Button::new("✔").small().selected(status.enabled()))
                                    .on_hover_text("replace filter")
                                    .clicked()
                                {
                                    self.state.tag_selection.clear();
                                    self.state.tag_selection.enable(tag.name());
                                }
                                if ui
                                    .add(egui::Button::new("+").small().selected(status.enabled()))
                                    .on_hover_text("add filter")
                                    .clicked()
                                {
                                    self.state.tag_selection.enable(tag.name());
                                }
                                if ui
                                    .add(egui::Button::new(" ").small())
                                    .on_hover_text("remove filter")
                                    .clicked()
                                {
                                    self.state.tag_selection.unselect(tag.name());
                                }
                                // if ui
                                //     .add(egui::Button::new("x").small().selected(status.disabled()))
                                //     .on_hover_text("filter on negation")
                                //     .clicked()
                                // {
                                //     self.state.tag_selection.disable(tag.name());
                                // }
                                ui.label("   ");
                                if ui.button("⟳").on_hover_text("refresh works").clicked() {
                                    self.sync.refresh_works_for_tag(tag.name()).ok();
                                }
                                ui.label("   ");
                                let content = if let Some(work_count) = tag.presumed_work_count() {
                                    format!(
                                        "{} ({} of {})",
                                        tag.name(),
                                        tag.actual_work_count(),
                                        work_count
                                    )
                                } else {
                                    format!("{} ({})", tag.name(), tag.actual_work_count())
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
                    Ok(())
                },
            )
            .inner?;
        Ok(())
    }

    fn show_works(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let works_count: usize = self
            .sync
            .pool_mut()
            .works_count(&self.state.tag_selection)?
            .try_into()?;
        if works_count == 0 {
            self.state.selected_work = WorkSelection::None;
        }
        ui.horizontal(|ui| {
            let mut remove = None;
            for enabled in self.state.tag_selection.enabled() {
                if ui
                    .button(format!("+{enabled}"))
                    .on_hover_text("Remove Filter")
                    .clicked()
                {
                    remove = Some(enabled.to_owned());
                }
            }
            if let Some(tag) = remove {
                self.state.tag_selection.disable(&tag);
            }
            if ui.button("x").clicked() {
                self.state.tag_selection.clear();
            }
            ui.label(format!("({works_count})"));
            ui.add(
                egui::Slider::new(&mut self.state.thumb_size, 64f32..=1024f32)
                    .text("Thumbnail Size")
                    .step_by(128.)
                    .fixed_decimals(0)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.3 })
                    .show_value(true)
                    .suffix("px"),
            );
        });
        if self.state.tag_selection.is_empty() {
            return Ok(());
        }

        let size = self.state.thumb_size;
        let width = ui.available_width();
        let n_wide = (width / size).floor().max(1.) as usize;
        let n_rows = works_count.div_ceil(n_wide);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, size, n_rows, |ui, rows| -> Result<()> {
                // Overfetch by 1x our current visible area in both directions so we can
                // usually scroll in either direction without pause or loading spinners.
                //
                //  All works (ideal case shown; the actual slice may go before or after)
                //  |--------<  [  ]  >--|
                //              [  ] <- visible slice
                //           |        | <- query slice
                //           |--[  ]--| <- works slice
                //
                //  What if we clip off the start or end
                //  |----------------< [ | ]
                //                     [   ] <- visible slice
                //                  |         | <- query slice
                //                     [ ] <- visible slice prime
                //                  |    | <- query slice prime
                //                  |--[ | - works slice
                let start_index = rows.start * n_wide;
                let end_index = rows.end * n_wide;
                let visible_slice = start_index..end_index;
                let query_slice = visible_slice.start.saturating_sub(visible_slice.len())
                    ..visible_slice.end + visible_slice.len();
                let visible_slice = visible_slice.start.clamp(0, works_count)
                    ..visible_slice.end.clamp(0, works_count);
                let query_slice =
                    query_slice.start.clamp(0, works_count)..query_slice.end.clamp(0, works_count);
                let works_slice =
                    visible_slice.start - query_slice.start..visible_slice.end - query_slice.start;

                // Attempt to query with the query slice, but things might have been added or
                // removed since we queried for the works_count.
                let query_start = Instant::now();
                let query_works = self
                    .sync
                    .pool_mut()
                    .works_list(query_slice.clone(), &self.state.tag_selection)?;
                self.state.perf.sample("Query Works", query_start.elapsed());

                // Pre-scan the works slice to ask to pre-load all the images that
                // are in our query window (Note: this extends outside the visible area
                // to make scrolling faster).
                let cache_start = Instant::now();
                for work in &query_works {
                    self.ensure_work_cached(ui.ctx(), work);
                }
                self.state
                    .perf
                    .sample("Cache Images", cache_start.elapsed());

                // Re-clamp the works slice after we fetch.
                let works_slice = works_slice.start.clamp(0, query_works.len())
                    ..works_slice.end.clamp(0, query_works.len());
                let mut work_offset = query_slice.start + works_slice.start;
                let visible_works = &query_works[works_slice];

                let sel_color = ui.style().visuals.selection.bg_fill;
                ui.style_mut().spacing.item_spacing = Vec2::ZERO;

                let draw_start = Instant::now();
                for row in visible_works.chunks(n_wide) {
                    ui.horizontal(|ui| {
                        for work in row {
                            // Selection uses the selection color for the background
                            // let is_selected = self.state.selected_work.is_selected(work.id());
                            let is_selected = self.state.selected_work.is_selected(work_offset);

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
                                    if ui.add(btn).clicked() {
                                        self.state.selected_work = WorkSelection::new(work_offset);
                                    }
                                });
                            });
                            work_offset += 1;
                        }
                    });
                }
                self.state.perf.sample("Draw Works", draw_start.elapsed());
                self.flush_works_lru(ui.ctx());
                Ok(())
            });

        Ok(())
    }

    fn show_info(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        if let Some(offset) = self.state.selected_work.get_selected_offset()
            && let Ok(work) = self
                .sync
                .pool_mut()
                .lookup_work_at_offset(offset, &self.state.tag_selection)
        {
            egui::Grid::new("work_info_grid").show(ui, |ui| {
                ui.label("Offset");
                ui.label(format!("{offset}"));
                ui.end_row();

                ui.label("Name");
                ui.label(work.name());
                ui.end_row();

                ui.label("Date");
                ui.label(format!("{}", work.date()));
            });
            ui.label(" ");
            ui.heading("Tags");
            ui.separator();
            for tag in work.tags() {
                if ui.button(tag).clicked() {
                    self.state.tag_selection.enable(tag);
                }
            }
        }
        self.state.perf.sample("Work Info", start.elapsed());
    }

    fn render_slideshow(&mut self, ctx: &egui::Context) -> Result<()> {
        // Bail back to the browser if we lose our selection.
        if !self.state.selected_work.has_selection() {
            self.state.mode = UxMode::Browser;
            return Ok(());
        }
        let work_offset = self
            .state
            .selected_work
            .get_selected_offset()
            .expect("no work selected in slideshow");
        let Ok(work) = self
            .sync
            .pool_mut()
            .lookup_work_at_offset(work_offset, &self.state.tag_selection)
        else {
            self.state.mode = UxMode::Browser;
            return Ok(());
        };
        let works_count = self
            .sync
            .pool_mut()
            .works_count(&self.state.tag_selection)?;

        egui::CentralPanel::default().show(ctx, |ui| {
            // See https://github.com/emilk/egui/blob/0f6310c598b5be92f339c9275a00d5decd838c1b/examples/custom_plot_manipulation/src/main.rs
            // for an example of how to do zoom and pan on a paint-like thing.

            let avail = ui.available_size();
            let img = self
                .get_best_image(&work, WorkSize::Screen)
                .show_loading_spinner(false)
                .maintain_aspect_ratio(true);

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
            ui.label(format!("{work_offset} of {works_count}"));
        });

        Ok(())
    }

    fn get_best_image<'b>(&'a self, work: &'b Work, req_sz: WorkSize) -> egui::Image<'b> {
        if matches!(req_sz, WorkSize::Screen) {
            let screen_path = get_data_path_for_url(self.data_dir, work.screen_url());
            let screen_uri = format!("file://{}", screen_path.display());
            if self.state.works_lru.contains(&screen_uri) {
                return egui::Image::new(screen_uri);
            }
            // Fall through to try to load the preview image
        }

        let preview_path = get_data_path_for_url(self.data_dir, work.preview_url());
        let preview_uri = format!("file://{}", preview_path.display());
        if self.state.works_lru.contains(&preview_uri) {
            egui::Image::new(preview_uri)
        } else {
            egui::Image::new(include_image!("../../assets/loading-preview.png"))
        }
    }

    fn ensure_work_cached(&mut self, ctx: &egui::Context, work: &Work) {
        // Limit number of times we call try_load_image per frame to prevent pauses
        if self.state.per_frame_work_upload_count > UxState::MAX_PER_FRAME_UPLOADS {
            return;
        }

        const SIZE_HINT: SizeHint = SizeHint::Size {
            width: 256,
            height: 256,
            maintain_aspect_ratio: true,
        };
        let screen_path = get_data_path_for_url(self.data_dir, work.screen_url());
        if screen_path.exists() {
            let screen_uri = format!("file://{}", screen_path.display());
            if !self.state.works_lru.contains(&screen_uri) {
                ctx.try_load_image(&screen_uri, SIZE_HINT).ok();
                self.state.per_frame_work_upload_count += 1;
            }
            self.state.works_lru.get_or_insert(screen_uri, || 0);
        }

        let preview_path = get_data_path_for_url(self.data_dir, work.preview_url());
        if preview_path.exists() {
            let preview_uri = format!("file://{}", preview_path.display());
            if !self.state.works_lru.contains(&preview_uri) {
                ctx.try_load_image(&preview_uri, SIZE_HINT).ok();
                self.state.per_frame_work_upload_count += 1;
            }
            self.state
                .works_lru
                .get_or_insert(preview_uri.clone(), || 0);
        }
    }

    fn flush_works_lru(&mut self, ctx: &egui::Context) {
        self.state.per_frame_work_upload_count = 0;
        while self.state.works_lru.len() > UxState::LRU_CACHE_SIZE {
            if let Some((uri, _)) = self.state.works_lru.pop_lru() {
                ctx.forget_image(&uri);
            }
        }
    }
}

impl TabViewer for SyncViewer<'_> {
    type Tab = TabMetadata;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        (tab.title.as_str()).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.title.as_str() {
            "Galleries" => self.show_galleries(ui).expect("failed to show gallery"),
            "Tags" => self.show_tags(ui).expect("failed to show tags"),
            "Works" => self.show_works(ui).expect("failed to show works"),
            "Work Info" => self.show_info(ui),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum UxMode {
    #[default]
    Browser,
    Slideshow,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UxToplevel {
    dock_state: DockState<TabMetadata>,
    state: UxState,
}

impl Default for UxToplevel {
    fn default() -> Self {
        let mut dock_state = DockState::new(vec![TabMetadata::new("Works")]);
        let surface = dock_state.main_surface_mut();
        let [right_node, galleries_node] =
            surface.split_left(NodeIndex::root(), 0.2, vec![TabMetadata::new("Galleries")]);
        let [_works_node, _info_node] =
            surface.split_right(right_node, 0.8, vec![TabMetadata::new("Work Info")]);
        surface.split_below(
            galleries_node,
            0.2,
            vec![TabMetadata::new("Tags"), TabMetadata::new("Artists")],
        );
        Self {
            dock_state,
            state: UxState::default(),
        }
    }
}

impl UxToplevel {
    pub fn main(
        &mut self,
        env: &Environment,
        host: &mut PluginHost,
        ctx: &egui::Context,
    ) -> Result<()> {
        let frame_start = Instant::now();

        match self.state.mode {
            UxMode::Browser => {
                self.render_menu(ctx);
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                    .show(ctx, |ui| {
                        DockArea::new(&mut self.dock_state)
                            .style(Style::from_egui(ui.style().as_ref()))
                            .show(
                                ctx,
                                &mut SyncViewer::wrap(host, &mut self.state, &env.data_dir()),
                            );
                    });

                self.render_preferences(ctx);
                self.render_performance(ctx);
                self.render_about(ctx);
            }
            UxMode::Slideshow => {
                SyncViewer::wrap(host, &mut self.state, &env.data_dir()).render_slideshow(ctx)?;
            }
        }

        self.handle_shortcuts(ctx);

        ctx.request_repaint_after(Duration::from_micros(1_000_000 / 60));

        self.state.perf.sample("Total", frame_start.elapsed());
        Ok(())
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut focus = None;
        ctx.memory(|mem| focus = mem.focused());

        const KEYS: [Key; 10] = [
            Key::Escape,
            Key::F1,
            Key::F3,
            Key::F11,
            Key::Space,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::F,
            Key::N,
            Key::P,
        ];
        let mut pressed = HashSet::new();
        ctx.input_mut(|input| {
            for key in &KEYS {
                if input.consume_key(Modifiers::NONE, *key) {
                    pressed.insert(*key);
                }
            }
        });

        // If a widget has focus, we generally don't want to do _anything_ with input except let the
        // user bail on that widget by pushing the escape button.
        if let Some(id) = focus
            && pressed.contains(&Key::Escape)
        {
            ctx.memory_mut(|mem| {
                mem.surrender_focus(id);
            });
            return;
        }

        // Each of the modes interprets keys a bit differently, out of necessity.
        match self.state.mode {
            UxMode::Browser => {
                if pressed.contains(&Key::F)
                    || pressed.contains(&Key::F11)
                    || pressed.contains(&Key::Space)
                {
                    if self.state.selected_work.has_selection() {
                        self.state.mode = UxMode::Slideshow;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                        return;
                    }
                } else if pressed.contains(&Key::Escape) {
                    if self.state.show_about {
                        self.state.show_about = false;
                    } else if self.state.show_performance {
                        self.state.show_performance = false;
                    } else if self.state.show_preferences {
                        self.state.show_preferences = false;
                    } else if pressed.contains(&Key::Escape) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else if pressed.contains(&Key::F3) {
                    self.state.show_performance = true;
                }
            }
            UxMode::Slideshow => {
                if pressed.contains(&Key::Escape) || pressed.contains(&Key::Space) {
                    self.state.mode = UxMode::Browser;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
            }
        }

        // Some keys work the same in any mode
        let pressed_left = pressed.contains(&Key::ArrowLeft) || pressed.contains(&Key::P);
        let pressed_right = pressed.contains(&Key::ArrowRight) || pressed.contains(&Key::N);
        if pressed_left {
            self.state.selected_work.move_to_prev();
        } else if pressed_right {
            self.state.selected_work.move_to_next();
        }
    }

    fn render_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.state.show_preferences = true;
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Performance...").clicked() {
                        self.state.show_performance = true;
                    }
                    ui.separator();
                    if ui.button("About...").clicked() {
                        self.state.show_about = true;
                    }
                });
            });
        });
    }

    fn render_preferences(&mut self, ctx: &egui::Context) {
        egui::Window::new("Preferences")
            .open(&mut self.state.show_preferences)
            .show(ctx, |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
            });
    }

    fn render_performance(&mut self, ctx: &egui::Context) {
        egui::Window::new("Performance")
            .open(&mut self.state.show_performance)
            .show(ctx, |ui| {
                self.state.perf.show(ui);
            });
    }

    fn render_about(&mut self, ctx: &egui::Context) {
        egui::Window::new("About")
            .open(&mut self.state.show_about)
            .show(ctx, |ui| {
                ui.label("about");
            });
    }
}
