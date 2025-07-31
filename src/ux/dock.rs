use crate::{
    shared::{performance::PerfTrack, progress::UpdateSource, update::DataUpdate},
    sync::{
        db::{reader::DbReadHandle, sync::DbSyncHandle},
        plugin::host::{PluginHandle, PluginHost},
    },
    ux::{db::UxDb, tag::UxTag, work::UxWork},
};
use anyhow::Result;
use egui::{self, Key, Margin, Modifiers, TextWrapMode};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use log::{Level, log};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path, time::Instant};
// use egui_video::{AudioDevice, Player};

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UxState {
    // Window and display state
    mode: UxMode,
    show_preferences: bool,
    show_performance: bool,
    show_about: bool,

    // Sub-UX
    db_ux: UxDb,
    tag_ux: UxTag,
    work_ux: UxWork,

    #[serde(skip)]
    perf: PerfTrack,
}

struct SyncViewer<'a> {
    sync: &'a mut PluginHost,
    state: &'a mut UxState,
    db: &'a DbReadHandle,
    db_sync: &'a DbSyncHandle,
}

impl<'a> SyncViewer<'a> {
    fn wrap(
        sync: &'a mut PluginHost,
        state: &'a mut UxState,
        db: &'a DbReadHandle,
        db_sync: &'a DbSyncHandle,
    ) -> Self {
        Self {
            sync,
            state,
            db,
            db_sync,
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

    fn show_plugins(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for plugin in self.sync.plugins_mut() {
                    ui.horizontal(|ui| {
                        ui.heading(plugin.name());
                        if ui.button("⟳ Tags").clicked() {
                            plugin.refresh_tags();
                        }
                        plugin.progress().ui(ui);
                    });
                    egui::Frame::new()
                        .inner_margin(indented(16))
                        .show(ui, |ui| {
                            Self::show_plugin_details(ui, plugin);
                            Self::show_plugin_tasks(ui, plugin);
                            Self::show_plugin_logs(ui, plugin);
                        });
                }
            });
    }

    fn show_database(&self, ui: &mut egui::Ui) {
        self.state.db_ux.ui(ui);
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        let mut tag_set = self.state.work_ux.tag_selection().to_owned();
        self.state.tag_ux.ui(&mut tag_set, self.sync, ui);
        self.state
            .work_ux
            .set_tag_selection(self.state.tag_ux.tags(), tag_set, self.db);
        self.state.perf.sample("Show Tags", start.elapsed());
    }

    fn show_works(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        self.state.work_ux.gallery_ui(
            self.state.tag_ux.tags(),
            self.db,
            self.db_sync,
            &mut self.state.perf,
            ui,
        );
        self.state.perf.sample("Show Works", start.elapsed());
    }

    fn show_info(&mut self, ui: &mut egui::Ui) {
        self.state
            .work_ux
            .info_ui(self.state.tag_ux.tags(), self.db, ui);
    }

    fn render_slideshow(&mut self, ctx: &egui::Context) {
        // Bail back to the browser if we lose our selection.
        if !self.state.work_ux.has_selection() {
            self.state.mode = UxMode::Browser;
            return;
        }
        self.state.work_ux.slideshow_ui(self.db_sync, ctx);
    }
}

impl TabViewer for SyncViewer<'_> {
    type Tab = TabMetadata;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        (tab.title.as_str()).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.title.as_str() {
            "Plugins" => self.show_plugins(ui),
            "Data" => self.show_database(ui),
            "Tags" => self.show_tags(ui),
            "Works" => self.show_works(ui),
            "Work Info" => self.show_info(ui),
            name => panic!("Unknown tab: {name}"),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum UxMode {
    #[default]
    Browser,
    Slideshow,
}

// fn init_audio_device() -> AudioDevice {
//     AudioDevice::new().expect("Failed to create audio output")
// }

#[derive(Serialize, Deserialize)]
pub struct UxToplevel {
    dock_state: DockState<TabMetadata>,
    state: UxState,
    errors: Vec<String>,
    // #[serde(skip, default = "init_audio_device")]
    // audio_device: AudioDevice,
    // #[serde(skip, default)]
    // video_player: Option<Player>,
}

impl Default for UxToplevel {
    fn default() -> Self {
        let mut dock_state = DockState::new(vec![TabMetadata::new("Works")]);
        let surface = dock_state.main_surface_mut();
        let [right_node, galleries_node] = surface.split_left(
            NodeIndex::root(),
            0.2,
            vec![TabMetadata::new("Plugins"), TabMetadata::new("Data")],
        );
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
            errors: Vec::new(),
            // audio_device: init_audio_device(),
            // video_player: None,
        }
    }
}

impl UxToplevel {
    pub fn startup(&mut self, data_dir: &Path, db: &DbReadHandle) {
        self.state.tag_ux.startup(db);
        self.state.work_ux.startup(data_dir, db);
    }

    pub fn handle_updates(&mut self, updates: &[DataUpdate], db: &DbReadHandle) {
        // self.state.plugin_ux.handle_updates(updates);
        self.state.db_ux.handle_updates(updates);
        self.state.tag_ux.handle_updates(db, updates);
        self.state
            .work_ux
            .handle_updates(self.state.tag_ux.tags(), db, updates);

        // Note: we need this to live above the dock impl for clarity, so do it here.
        for update in updates {
            if let DataUpdate::Log {
                source: UpdateSource::Unknown,
                level,
                message,
            } = update
            {
                log!(*level, "{message}");
                self.errors.push(message.to_owned());
            }
        }
    }

    pub fn main(
        &mut self,
        db: &DbReadHandle,
        db_sync: &DbSyncHandle,
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
                        // Show errors above everything else
                        let mut remove = None;
                        for (offset, message) in self.errors.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ui.small_button("x").clicked() {
                                    remove = Some(offset);
                                }
                                ui.label(message);
                            });
                        }
                        if let Some(offset) = remove {
                            self.errors.remove(offset);
                        }

                        // Show the main dock area
                        DockArea::new(&mut self.dock_state)
                            .style(Style::from_egui(ui.style().as_ref()))
                            .show(
                                ctx,
                                &mut SyncViewer::wrap(host, &mut self.state, db, db_sync),
                            );
                    });

                // Show any windows that are open
                self.render_preferences(ctx);
                self.render_performance(ctx);
                self.render_about(ctx);
            }
            UxMode::Slideshow => {
                SyncViewer::wrap(host, &mut self.state, db, db_sync).render_slideshow(ctx);
            }
        }

        self.handle_shortcuts(ctx);

        // ctx.request_repaint_after(Duration::from_micros(1_000_000 / 60));

        self.state.perf.sample("Total", frame_start.elapsed());
        Ok(())
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut focus = None;
        ctx.memory(|mem| focus = mem.focused());

        const KEYS: [Key; 9] = [
            Key::Escape,
            Key::F1,
            Key::F3,
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
            return;
        }

        // Each of the modes interprets keys a bit differently, out of necessity.
        match self.state.mode {
            UxMode::Browser => {
                if pressed.contains(&Key::F11) || pressed.contains(&Key::Space) {
                    if self.state.work_ux.has_selection() {
                        self.state.mode = UxMode::Slideshow;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
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
