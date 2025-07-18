use crate::{
    shared::{environment::Environment, progress::Progress, tag::TagSet},
    sync::plugin::{client::get_data_path_for_url, host::PluginHost},
};
use anyhow::Result;
use artchiver_sdk::Work;
use egui::{self, Key, Margin, Modifiers, Sense, SizeHint, TextWrapMode, Vec2};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum WorkSelection {
    #[default]
    None,
    Work {
        work_id: i64,
        offset: usize,
    },
}

impl WorkSelection {
    pub fn new(work_id: i64, offset: usize) -> Self {
        Self::Work { work_id, offset }
    }

    pub fn is_selected(&self, work_id: i64) -> bool {
        match self {
            Self::None => false,
            Self::Work { work_id: id, .. } => *id == work_id,
        }
    }

    pub fn get_selected(&self) -> Option<i64> {
        match self {
            Self::None => None,
            Self::Work { work_id, .. } => Some(*work_id),
        }
    }

    pub fn get_offset(&self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Work { offset, .. } => Some(*offset),
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
    show_about: bool,
    tag_filter: String,
    tag_selection: TagSet,
    selected_work: WorkSelection,

    #[serde(default = "LruCache::unbounded")]
    #[serde(skip)]
    works_lru: LruCache<String, u32>,

    #[serde(skip)]
    per_frame_work_upload_count: usize,
}

impl Default for UxState {
    fn default() -> Self {
        Self {
            mode: UxMode::Browser,
            show_preferences: false,
            show_about: false,
            tag_filter: String::new(),
            tag_selection: TagSet::default(),
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

    fn show_galleries(&mut self, ui: &mut egui::Ui) -> Result<()> {
        egui::ScrollArea::vertical()
            .show(ui, |ui| -> Result<()> {
                for plugin in self.sync.plugins_mut() {
                    ui.horizontal(|ui| {
                        ui.heading(plugin.name());
                        if ui.button("⟳ Tags").clicked() {
                            plugin.refresh_tags().ok();
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
                                                for (config_key, config_val) in
                                                    meta.configurations_mut()
                                                {
                                                    ui.label(config_key);
                                                    ui.text_edit_singleline(config_val);
                                                    ui.end_row();
                                                }
                                                if !meta.configurations().is_empty()
                                                    && ui.button("Update").clicked()
                                                {
                                                    plugin.apply_configuration()?;
                                                }
                                            }
                                            Ok(())
                                        })
                                        .inner?;
                                    Ok(())
                                });
                            egui::CollapsingHeader::new("Messages")
                                .id_salt(format!("messages_section_{}", plugin.name()))
                                .show(ui, |ui| {
                                    for message in plugin.messages() {
                                        ui.add(
                                            egui::Label::new(message)
                                                .wrap_mode(TextWrapMode::Truncate),
                                        );
                                    }
                                });
                            egui::CollapsingHeader::new("Traces")
                                .id_salt(format!("traces_section_{}", plugin.name()))
                                .show(ui, |ui| {
                                    for message in plugin.traces() {
                                        ui.add(
                                            egui::Label::new(message)
                                                .wrap_mode(TextWrapMode::Truncate),
                                        );
                                    }
                                });
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
        let tag_cnt = self.sync.pool_mut().tags_count(&self.state.tag_filter)?;
        // Show the filter and global refresh-all-tags button.
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.state.tag_filter);
            if ui.button("x").clicked() {
                self.state.tag_filter.clear();
            }
            ui.label(format!("({tag_cnt})",));
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
                    let tags = self
                        .sync
                        .pool_mut()
                        .tags_list(row_range, &self.state.tag_filter)?;

                    egui::Grid::new("tag_grid")
                        .num_columns(1)
                        .spacing([0., 0.])
                        .min_col_width(0.)
                        .show(ui, |ui| {
                            for tag in tags {
                                let status = self.state.tag_selection.status(tag.name());
                                if ui
                                    .add(egui::Button::new("✔").small().selected(status.enabled()))
                                    .clicked()
                                {
                                    self.state.tag_selection.enable(tag.name());
                                }
                                if ui.add(egui::Button::new(" ").small()).clicked() {
                                    self.state.tag_selection.unselect(tag.name());
                                }
                                if ui
                                    .add(egui::Button::new("x").small().selected(status.disabled()))
                                    .clicked()
                                {
                                    self.state.tag_selection.disable(tag.name());
                                }
                                ui.label("   ");
                                if ui.button("⟳").clicked() {
                                    self.sync.refresh_works_for_tag(tag.name()).ok();
                                }
                                ui.label("   ");
                                let content = format!("{} ({})", tag.name(), tag.work_count());
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
        ui.horizontal(|ui| {
            ui.heading(self.state.tag_selection.to_string());
            ui.label(format!("({works_count})"));
        });

        const PREVIEW_SIZE: f32 = 256.;
        const SIZE: f32 = PREVIEW_SIZE;

        let width = ui.available_width();
        let n_wide = (width / SIZE).floor().max(1.) as usize;
        let n_rows = works_count.div_ceil(n_wide);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, SIZE, n_rows, |ui, rows| -> Result<()> {
                let start_index = rows.start * n_wide;
                let end_index = (rows.end * n_wide).min(works_count);
                let range_len = end_index - start_index;

                // Note: overfetch by 1x our current visible area in both directions so we can
                //       usually scroll in either direction without pause or loading spinners.
                let works_range = start_index.saturating_sub(range_len)
                    ..end_index.saturating_add(range_len).min(works_count);
                let works = self
                    .sync
                    .pool_mut()
                    .works_list(works_range.clone(), &self.state.tag_selection)?;

                let sel_color = ui.style().visuals.selection.bg_fill;
                ui.style_mut().spacing.item_spacing = Vec2::ZERO;

                // We subtracted off range_len, but may have clipped with zero, so we have to reconstruct.
                let works_start = start_index - works_range.start;
                let mut offset = works_start;
                for row in works[works_start..end_index].chunks(n_wide) {
                    ui.horizontal(|ui| {
                        for work in row {
                            if let Some(uri) = self.ensure_work_cached(work, ui.ctx()) {
                                // Selection uses the selection color for the background
                                let is_selected = self.state.selected_work.is_selected(work.id());

                                let img = egui::Image::new(uri)
                                    .alt_text(work.name())
                                    .show_loading_spinner(true)
                                    .maintain_aspect_ratio(true);

                                let mut pad = 0.;
                                let mut inner_margin = Margin::ZERO;
                                if let Some(size) = img
                                    .load_and_calc_size(ui, Vec2::new(PREVIEW_SIZE, PREVIEW_SIZE))
                                {
                                    // Wide things are already centered for some reason,
                                    // so we only need to care about tall images
                                    if size.y > size.x {
                                        pad = (SIZE - size.x) / 2.;
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
                                    .min_size(Vec2::new(SIZE - pad, SIZE))
                                    .max_size(Vec2::new(SIZE - pad, SIZE))
                                    .default_size(Vec2::new(SIZE - pad, SIZE))
                                    .resizable([false, false]);

                                frm.show(ui, |ui| {
                                    rsz.show(ui, |ui| {
                                        if ui.add(btn).clicked() {
                                            self.state.selected_work =
                                                WorkSelection::new(work.id(), offset);
                                        }
                                    });
                                });
                            } else {
                                ui.add(egui::Spinner::new().size(SIZE));
                            }
                            offset += 1;
                        }
                    });
                }
                self.flush_works_lru(ui.ctx());
                Ok(())
            });
        Ok(())
    }

    fn show_info(&mut self, ui: &mut egui::Ui) {
        if let Some(work_id) = self.state.selected_work.get_selected()
            && let Ok(work) = self.sync.pool_mut().lookup_work(work_id)
        {
            egui::Grid::new("work_info_grid").show(ui, |ui| {
                ui.label("Name");
                ui.label(work.name());
                ui.end_row();
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
    }

    fn render_slideshow(&mut self, ctx: &egui::Context) -> Result<()> {
        if !self.state.selected_work.has_selection() {
            self.state.mode = UxMode::Browser;
            return Ok(());
        }
        let work_id = self
            .state
            .selected_work
            .get_selected()
            .expect("selected work");
        let work = self.sync.pool_mut().lookup_work(work_id)?;

        egui::CentralPanel::default().show(ctx, |ui| {
            let size = ui.available_size();
            if let Some(uri) = self.ensure_work_cached(&work, ui.ctx()) {
                ui.image(uri);
            } else {
                ui.add(egui::Spinner::new().size(size.x.min(size.y)));
            }
        });

        Ok(())
    }

    fn ensure_work_cached(&mut self, work: &Work, ctx: &egui::Context) -> Option<String> {
        // Limit number of times we call try_load_image per frame to prevent pauses
        if self.state.per_frame_work_upload_count > UxState::MAX_PER_FRAME_UPLOADS {
            return None;
        }

        let size_hint = SizeHint::Size {
            width: 256,
            height: 256,
            maintain_aspect_ratio: true,
        };
        let screen_path = get_data_path_for_url(self.data_dir, work.screen_url());
        if screen_path.exists() {
            let screen_uri = format!("file://{}", screen_path.display());
            if !self.state.works_lru.contains(&screen_uri) {
                ctx.try_load_image(&screen_uri, size_hint).ok();
                self.state.per_frame_work_upload_count += 1;
            }
            self.state.works_lru.get_or_insert(screen_uri, || 0);
        }

        let preview_path = get_data_path_for_url(self.data_dir, work.preview_url());
        if preview_path.exists() {
            let preview_uri = format!("file://{}", preview_path.display());
            if !self.state.works_lru.contains(&preview_uri) {
                ctx.try_load_image(&preview_uri, size_hint).ok();
                self.state.per_frame_work_upload_count += 1;
            }
            self.state
                .works_lru
                .get_or_insert(preview_uri.clone(), || 0);
            Some(preview_uri)
        } else {
            None
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
                self.render_about(ctx);
            }
            UxMode::Slideshow => {
                SyncViewer::wrap(host, &mut self.state, &env.data_dir()).render_slideshow(ctx)?;
            }
        }

        self.handle_shortcuts(host, ctx).expect("failed to handle shortcuts");

        Ok(())
    }

    fn handle_shortcuts(&mut self, host: &mut PluginHost, ctx: &egui::Context) -> Result<()> {
        let mut focus = None;
        ctx.memory(|mem| focus = mem.focused());

        const KEYS: [Key; 9] = [
            Key::Escape,
            Key::F1,
            Key::F,
            Key::F11,
            Key::Space,
            Key::ArrowLeft,
            Key::ArrowRight,
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
            return Ok(());
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
                        return Ok(());
                    }
                } else if pressed.contains(&Key::Escape) {
                    if self.state.show_about {
                        self.state.show_about = false;
                    } else if self.state.show_preferences {
                        self.state.show_preferences = false;
                    } else if pressed.contains(&Key::Escape) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            UxMode::Slideshow => {
                if pressed.contains(&Key::Escape) {
                    self.state.mode = UxMode::Browser;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
            }
        }

        // Some keys work the same in any mode
        if let Some(mut offset) = self.state.selected_work.get_offset() {
            let pressed_left = pressed.contains(&Key::ArrowLeft) || pressed.contains(&Key::P);
            let pressed_right = pressed.contains(&Key::ArrowRight)
                || pressed.contains(&Key::N)
                || pressed.contains(&Key::Space);
            if pressed_left || pressed_right {
                let count: usize = host
                    .pool_mut()
                    .works_count(&self.state.tag_selection)?
                    .try_into()?;

                if pressed_right {
                    if offset >= count - 1 {
                        offset = 0;
                    } else {
                        offset += 1;
                    }
                } else if pressed_left {
                    if offset == 0 {
                        offset = count - 1;
                    } else {
                        offset -= 1;
                    }
                }

                if let Ok(works) = host
                    .pool_mut()
                    .works_list(offset..offset + 1, &self.state.tag_selection)
                    && let Some(work) = works.first()
                {
                    self.state.selected_work = WorkSelection::new(work.id(), offset);
                }
            }
        }

        Ok(())
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

    fn render_about(&mut self, ctx: &egui::Context) {
        egui::Window::new("About")
            .open(&mut self.state.show_about)
            .show(ctx, |ui| {
                ui.label("about");
            });
    }
}
