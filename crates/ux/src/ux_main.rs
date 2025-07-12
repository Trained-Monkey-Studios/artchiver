use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use sync::{Progress, SyncEngine};

// Utility function to get an egui margin inset from the left.
fn indented(px: i8) -> egui::Margin {
    let mut m = egui::Margin::ZERO;
    m.left = px;
    m
}

pub struct UxPlugin;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, SystemSet)]
pub enum UxSet {
    Main,
}

impl Plugin for UxPlugin {
    fn build(&self, app: &mut App) {
        assert!(app.is_plugin_added::<EguiPlugin>());
        app.insert_resource(UxToplevel::default())
            .add_systems(EguiPrimaryContextPass, ux_main.in_set(UxSet::Main));
    }
}

pub struct TabMetadata {
    title: String,
}

impl TabMetadata {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
        }
    }
}

struct SyncViewer<'a> {
    sync: &'a mut SyncEngine,
    state: &'a mut UxState,
}

impl<'a> SyncViewer<'a> {
    fn wrap(sync: &'a mut SyncEngine, state: &'a mut UxState) -> SyncViewer<'a> {
        Self { sync, state }
    }

    fn show_plugins(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for plugin in self.sync.plugins() {
                ui.horizontal(|ui| {
                    ui.heading(plugin.name());
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
                    .show(ui, |ui| {
                        ui.label(plugin.description());
                        egui::Grid::new("plugin_grid")
                            .num_columns(2)
                            .show(ui, |ui| {
                                ui.label("Source");
                                ui.label(plugin.source().display().to_string());
                                ui.end_row();
                                ui.label("Version");
                                ui.label(plugin.version());
                                ui.end_row();
                            });
                        ui.collapsing("Messages", |ui| {
                            for message in plugin.messages() {
                                ui.label(message);
                            }
                        });
                    });
            }
        });
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.state.tag_filter);
            if ui.button("Refresh All").clicked() {
                self.sync.refresh_tags().ok();
            }
        });
        let start = std::time::Instant::now();
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        let tag_cnt = self.sync.pool().count_tags(&self.state.tag_filter).unwrap();
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(
                ui,
                row_height,
                tag_cnt.try_into().unwrap(),
                |ui, row_range| {
                    let tags = self
                        .sync
                        .pool()
                        .list_tags(row_range, &self.state.tag_filter)
                        .unwrap();
                    for tag in tags {
                        ui.label(tag);
                    }
                },
            );
        println!("tag draw: {:?}", start.elapsed());
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
            "Tags" => self.show_tags(ui),
            _ => {}
        }
    }
}

#[derive(Default)]
pub struct UxState {
    show_preferences: bool,
    show_about: bool,
    tag_filter: String,
}

#[derive(Resource)]
pub struct UxToplevel {
    dock_state: DockState<TabMetadata>,
    state: UxState,
}

impl Default for UxToplevel {
    fn default() -> Self {
        let mut dock_state = DockState::new(vec![TabMetadata::new("Works")]);
        let surface = dock_state.main_surface_mut();
        let [_works_node, plugins_node] =
            surface.split_left(NodeIndex::root(), 0.2, vec![TabMetadata::new("Plugins")]);
        surface.split_below(
            plugins_node,
            0.2,
            vec![TabMetadata::new("Tags"), TabMetadata::new("Artists")],
        );
        Self {
            dock_state,
            state: Default::default(),
        }
    }
}

fn ux_main(
    mut contexts: EguiContexts,
    mut ux: ResMut<UxToplevel>,
    mut sync: ResMut<SyncEngine>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut app_exit: EventWriter<AppExit>,
) -> Result {
    if keyboard.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
    ux.main(&mut sync, contexts.ctx_mut()?)
}

impl UxToplevel {
    pub fn main(&mut self, sync: &mut SyncEngine, ctx: &mut egui::Context) -> Result {
        // Menu Bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.state.show_preferences = true;
                        ui.close_menu();
                    }

                    // NOTE: no File->Quit on web pages!
                    let is_web = cfg!(target_arch = "wasm32");
                    if !is_web {
                        ui.separator();
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About...").clicked() {
                        self.state.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        // Preferences
        if self.state.show_preferences {
            egui::Window::new("Preferences").show(ctx, |ui| {
                self.render_preferences(ui);
            });
        }

        // About
        if self.state.show_about {
            egui::Window::new("About").show(ctx, |ui| {
                self.render_about(ui);
            });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
            .show(ctx, |ui| {
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show(ctx, &mut SyncViewer::wrap(sync, &mut self.state));
            });

        Ok(())
    }

    fn render_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label("preferences");
    }

    fn render_about(&mut self, ui: &mut egui::Ui) {
        ui.label("about");
    }
}
