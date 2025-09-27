use crate::db::writer::DbWriteHandle;
use crate::{
    db::reader::DbReadHandle,
    plugin::host::PluginHost,
    shared::{performance::PerfTrack, progress::UpdateSource, update::DataUpdate},
    ux::{
        db::UxDb,
        plugin::UxPlugin,
        tag::UxTag,
        theme::Theme,
        tutorial::{Tutorial, TutorialStep},
        work::UxWork,
    },
};
use anyhow::Result;
use egui::{self, Key, Modifiers};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use log::log;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path, time::Instant};

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

#[derive(Default, Serialize, Deserialize)]
pub struct UxState {
    // Window and display state
    mode: UxMode,
    show_preferences: bool,
    show_performance: bool,
    show_about: bool,
    tutorial_step: TutorialStep,

    // Preferences
    theme: Theme,

    // Sub-UX
    db_ux: UxDb,
    plugin_ux: UxPlugin,
    tag_ux: UxTag,
    work_ux: UxWork,

    #[serde(skip)]
    perf: PerfTrack,
}

impl UxState {
    // Note: we have to punch this through to get joint mutable access to self for work's
    // tag_selection and the tag_ux state.
    fn show_tags_list(
        &mut self,
        host: &mut PluginHost,
        db_write: &DbWriteHandle,
        ui: &mut egui::Ui,
    ) {
        self.tag_ux.ui(
            self.work_ux.tag_selection_mut(),
            host,
            Tutorial::new(&mut self.tutorial_step, &self.theme, ui.style().clone()),
            db_write,
            ui,
        );
    }
}

struct SyncViewer<'a> {
    sync: &'a mut PluginHost,
    state: &'a mut UxState,
    #[expect(unused)]
    db_read: &'a DbReadHandle,
    db_write: &'a DbWriteHandle,
}

impl<'a> SyncViewer<'a> {
    fn wrap(
        sync: &'a mut PluginHost,
        state: &'a mut UxState,
        db_read: &'a DbReadHandle,
        db_write: &'a DbWriteHandle,
    ) -> Self {
        Self {
            sync,
            state,
            db_read,
            db_write,
        }
    }

    fn show_plugins(&mut self, ui: &mut egui::Ui) {
        self.state.plugin_ux.ui(
            self.sync,
            Tutorial::new(
                &mut self.state.tutorial_step,
                &self.state.theme,
                ui.style().clone(),
            ),
            ui,
        );
    }

    fn show_database(&self, ui: &mut egui::Ui) {
        self.state.db_ux.ui(ui);
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        self.state.show_tags_list(self.sync, self.db_write, ui);
        self.state.perf.sample("Show Tags", start.elapsed());
    }

    fn show_works(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        self.state.work_ux.gallery_ui(
            self.state.tag_ux.tags(),
            self.db_write,
            &mut self.state.perf,
            ui,
        );
        self.state.perf.sample("Show Works", start.elapsed());
    }

    fn show_info(&mut self, ui: &mut egui::Ui) {
        self.state
            .work_ux
            .info_ui(self.state.tag_ux.tags(), self.db_write, self.sync, ui);
    }

    fn render_slideshow(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Bail back to the browser if we lose our selection.
        if !self.state.work_ux.has_selection() {
            self.state.mode = UxMode::Browser;
            return;
        }
        self.state
            .work_ux
            .slideshow_ui(self.state.tag_ux.tags(), self.db_write, ctx, frame);
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
            "Artists" => {
                // TODO: implement artists too!
                ui.label("TODO");
            }
            name => panic!("Unknown tab: {name}"),
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        true
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum UxMode {
    #[default]
    Browser,
    Slideshow,
}

#[derive(Serialize, Deserialize)]
pub struct UxToplevel {
    dock_state: DockState<TabMetadata>,
    state: UxState,
    errors: Vec<String>,
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
            0.4,
            vec![TabMetadata::new("Tags"), TabMetadata::new("Artists")],
        );
        Self {
            dock_state,
            state: UxState::default(),
            errors: Vec::new(),
        }
    }
}

impl UxToplevel {
    pub fn startup(
        &mut self,
        ctx: &egui::Context,
        data_dir: &Path,
        db: &DbReadHandle,
        cc: &eframe::CreationContext<'_>,
    ) {
        self.state.theme.apply(ctx);
        self.state.tag_ux.startup(db);
        self.state
            .work_ux
            .startup(data_dir, db, cc)
            .expect("Failed to load works ui");
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

    pub fn draw(
        &mut self,
        db: &DbReadHandle,
        db_write: &DbWriteHandle,
        host: &mut PluginHost,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
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
                                &mut SyncViewer::wrap(host, &mut self.state, db, db_write),
                            );
                    });

                // Show any windows that are open
                self.render_tutorial(ctx);
                self.render_preferences(ctx);
                self.render_performance(ctx);
                self.render_about(ctx);
            }
            UxMode::Slideshow => {
                SyncViewer::wrap(host, &mut self.state, db, db_write).render_slideshow(ctx, frame);
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
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else if pressed.contains(&Key::F3) {
                    self.state.show_performance = true;
                }
            }
            UxMode::Slideshow => {
                if pressed.contains(&Key::Escape) || pressed.contains(&Key::Space) {
                    self.state.work_ux.on_leave_slideshow();
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
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.state.show_preferences = true;
                    }
                });
                ui.menu_button("View", |ui| {
                    const TABS: [&str; 6] =
                        ["Plugins", "Tags", "Works", "Work Info", "Artists", "Data"];
                    let mut have_section = false;
                    for name in &TABS {
                        let closed = self
                            .dock_state
                            .find_tab_from(|tab| &tab.title == name)
                            .is_none();
                        if closed {
                            have_section = true;
                            if ui.button("Plugins").clicked() {
                                self.dock_state.push_to_focused_leaf(TabMetadata::new(name));
                            }
                        }
                    }
                    if have_section {
                        ui.separator();
                    }
                    if ui.button("Performance Monitor...").clicked() {
                        self.state.show_performance = true;
                    }
                });
                ui.menu_button("Help", |ui| {
                    if self.state.tutorial_step != TutorialStep::Beginning
                        && ui.button("Restart Tutorial...").clicked()
                    {
                        self.state.tutorial_step = TutorialStep::Beginning;
                    }
                    ui.separator();
                    if ui.button("About...").clicked() {
                        self.state.show_about = true;
                    }
                });
            });
        });
    }

    fn render_tutorial(&mut self, ctx: &egui::Context) {
        if self.state.tutorial_step == TutorialStep::Beginning {
            egui::Window::new("Welcome to Artchiver")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .auto_sized()
                .show(ctx, |ui| {
                    ui.heading("Welcome to Artchiver");
                    ui.separator();
                    ui.label("Artchiver will help you download, browse, and enjoy the world's art, from classical paintings to podcasts.");
                    ui.label("");
                    ui.label("Artchiver is streamlined for efficient search and browsing rather than discoverability, so it can be a bit intimidating at first.");
                    ui.label("");
                    ui.label("This tutorial will show you the ropes and get you up to speed fast.");
                    ui.label("");
                    self.state.theme.ui(ui);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Learn More").clicked() {
                            self.state.tutorial_step = TutorialStep::PluginsIntro;
                        }
                        if ui.button("Skip Tutorial").clicked() {
                            self.state.tutorial_step = TutorialStep::Finished;
                        }
                    })
                });
        }
    }

    fn render_preferences(&mut self, ctx: &egui::Context) {
        egui::Window::new("Preferences")
            .open(&mut self.state.show_preferences)
            .show(ctx, |ui| {
                self.state.theme.ui(ui);
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
