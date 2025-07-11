use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use sync::SyncEngine;

pub struct UxPlugin;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, SystemSet)]
pub enum UxSet {
    Main,
}

impl Plugin for UxPlugin {
    fn build(&self, app: &mut App) {
        assert!(app.is_plugin_added::<EguiPlugin>());
        app.insert_resource(UxState::default())
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
}

impl<'a> SyncViewer<'a> {
    fn wrap(sync: &'a mut SyncEngine) -> SyncViewer<'a> {
        Self { sync }
    }

    fn show_plugins(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for plugin in self.sync.plugins() {
                ui.collapsing(plugin.name(), |ui| {
                    let desc = plugin
                        .metadata()
                        .map(|m| m.description())
                        .unwrap_or_else(|| "not yet loaded");
                    let version = plugin
                        .metadata()
                        .map(|m| m.version())
                        .unwrap_or_else(|| "not yet loaded");
                    ui.label(desc);
                    ui.label(format!("Source: {}", plugin.source().display()));
                    ui.label(format!("Version: {version}"));
                    ui.separator();
                    for message in plugin.messages().unwrap().iter() {
                        ui.label(message);
                    }
                });
            }
        });
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) {
        if ui.button("Refresh Tags").clicked() {
            self.sync.refresh_tags().ok();
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
            "Plugins" => self.show_plugins(ui),
            "Tags" => self.show_tags(ui),
            _ => {}
        }
    }
}

#[derive(Resource)]
pub struct UxState {
    show_preferences: bool,
    show_about: bool,
    dock_state: DockState<TabMetadata>,
}

impl Default for UxState {
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
            show_about: false,
            show_preferences: false,
            dock_state,
        }
    }
}

fn ux_main(
    mut contexts: EguiContexts,
    mut ux: ResMut<UxState>,
    mut sync: ResMut<SyncEngine>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut app_exit: EventWriter<AppExit>,
) -> Result {
    if keyboard.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
    ux.main(&mut sync, contexts.ctx_mut()?)
}

impl UxState {
    pub fn main(&mut self, sync: &mut SyncEngine, ctx: &mut egui::Context) -> Result {
        // Menu Bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.show_preferences = true;
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
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        // Preferences
        if self.show_preferences {
            egui::Window::new("Preferences").show(ctx, |ui| {
                self.render_preferences(ui);
            });
        }

        // About
        if self.show_about {
            egui::Window::new("About").show(ctx, |ui| {
                self.render_about(ui);
            });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
            .show(ctx, |ui| {
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show(ctx, &mut SyncViewer::wrap(sync));
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
