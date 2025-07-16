use artchiver_sdk::Work;
use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass,
    egui::{self, SizeHint, TextWrapMode},
};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use lru::LruCache;
use std::path::Path;
use sync::{Environment, PluginHost, Progress, TagSet, get_data_path_for_url};

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
    sync: &'a mut PluginHost,
    state: &'a mut UxState,
    data_dir: &'a Path,
}

impl<'a> SyncViewer<'a> {
    fn wrap(
        sync: &'a mut PluginHost,
        state: &'a mut UxState,
        data_dir: &'a Path,
    ) -> SyncViewer<'a> {
        Self {
            sync,
            state,
            data_dir,
        }
    }

    fn show_galleries(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for plugin in self.sync.plugins_mut() {
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
                        egui::CollapsingHeader::new("Details")
                            .id_salt(format!("details_section_{}", plugin.name()))
                            .show(ui, |ui| {
                                ui.label(plugin.description());
                                egui::Grid::new(format!("plugin_grid_{}", plugin.name()))
                                    .num_columns(2)
                                    .show(ui, |ui| {
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
                                                plugin.apply_configuration().unwrap();
                                            }
                                        }
                                    });
                            });
                        egui::CollapsingHeader::new("Messages")
                            .id_salt(format!("messages_section_{}", plugin.name()))
                            .show(ui, |ui| {
                                for message in plugin.messages() {
                                    ui.add(
                                        egui::Label::new(message).wrap_mode(TextWrapMode::Truncate),
                                    );
                                }
                            });
                        egui::CollapsingHeader::new("Traces")
                            .id_salt(format!("traces_section_{}", plugin.name()))
                            .show(ui, |ui| {
                                for message in plugin.traces() {
                                    ui.add(
                                        egui::Label::new(message).wrap_mode(TextWrapMode::Truncate),
                                    );
                                }
                            });
                    });
            }
        });
    }

    fn show_tags(&mut self, ui: &mut egui::Ui) {
        let tag_cnt = self
            .sync
            .pool_mut()
            .tags_count(&self.state.tag_filter)
            .unwrap();
        // Show the filter and global refresh-all-tags button.
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.state.tag_filter);
            ui.label(format!("({tag_cnt})",));
            if ui.button("⟳ Refresh All").clicked() {
                self.sync.refresh_tags().ok();
            }
        });
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(
                ui,
                row_height,
                tag_cnt.try_into().unwrap(),
                |ui, row_range| {
                    let tags = self
                        .sync
                        .pool_mut()
                        .tags_list(row_range, &self.state.tag_filter)
                        .unwrap();

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
                },
            );
    }

    fn show_works(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.state.tag_selection.to_string());
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.state.work_filter);
            ui.label("UNKNOWN COUNT");
        });

        let works = self
            .sync
            .pool_mut()
            .works_list(&self.state.tag_selection)
            .unwrap();
        ui.horizontal_wrapped(|ui| {
            for work in works {
                if let Some(uri) = self.ensure_work_cached(&work, ui.ctx()) {
                    let img = egui::Image::new(uri)
                        .alt_text(work.name())
                        .show_loading_spinner(true)
                        .maintain_aspect_ratio(true)
                        .fit_to_exact_size(egui::Vec2::new(256., 256.));
                    if ui.add(img).on_hover_text(work.name()).clicked() {
                        println!("work: {}", work.name());
                    }
                } else {
                    ui.add(egui::Spinner::new().size(256.));
                }
            }
            self.flush_works_lru();
        });
    }

    fn ensure_work_cached(&mut self, work: &Work, ctx: &egui::Context) -> Option<String> {
        let screen_path = get_data_path_for_url(self.data_dir, work.screen_url()).unwrap();
        let screen_uri = format!("file://{}", screen_path.display());
        if screen_path.exists() {
            let _ = ctx.try_load_image(&screen_uri, SizeHint::Size(256, 256));
            self.state.works_lru.get_or_insert(screen_uri.clone(), || 0);
        }

        let preview_path = get_data_path_for_url(self.data_dir, work.preview_url()).unwrap();
        let preview_uri = format!("file://{}", preview_path.display());
        if preview_path.exists() {
            let _ = ctx.try_load_image(&preview_uri, SizeHint::Size(256, 256));
            self.state
                .works_lru
                .get_or_insert(preview_uri.clone(), || 0);
            Some(preview_uri)
        } else {
            None
        }
    }

    fn flush_works_lru(&mut self) {
        while self.state.works_lru.len() > UxState::LRU_CACHE_SIZE {
            let _ = self.state.works_lru.pop_lru();
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
            "Galleries" => self.show_galleries(ui),
            "Tags" => self.show_tags(ui),
            "Works" => self.show_works(ui),
            _ => {}
        }
    }
}

pub struct UxState {
    show_preferences: bool,
    show_about: bool,
    tag_filter: String,
    tag_selection: TagSet,
    work_filter: String,
    works_lru: LruCache<String, u32>,
}

impl UxState {
    const LRU_CACHE_SIZE: usize = 1_000;

    pub fn new() -> Self {
        Self {
            show_preferences: false,
            show_about: false,
            tag_filter: String::new(),
            tag_selection: TagSet::default(),
            work_filter: String::new(),
            works_lru: LruCache::unbounded(),
        }
    }
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
        let [_works_node, galleries_node] =
            surface.split_left(NodeIndex::root(), 0.2, vec![TabMetadata::new("Galleries")]);
        surface.split_below(
            galleries_node,
            0.2,
            vec![TabMetadata::new("Tags"), TabMetadata::new("Artists")],
        );
        Self {
            dock_state,
            state: UxState::new(),
        }
    }
}

fn ux_main(
    mut contexts: EguiContexts,
    mut ux: ResMut<UxToplevel>,
    mut sync: ResMut<PluginHost>,
    env: Res<Environment>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut app_exit: EventWriter<AppExit>,
) -> Result {
    if keyboard.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
    ux.main(&env, &mut sync, contexts.ctx_mut()?, &mut app_exit)
}

impl UxToplevel {
    pub fn main(
        &mut self,
        env: &Environment,
        sync: &mut PluginHost,
        ctx: &mut egui::Context,
        app_exit: &mut EventWriter<AppExit>,
    ) -> Result {
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
                            app_exit.write(AppExit::Success);
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
                    .show(
                        ctx,
                        &mut SyncViewer::wrap(sync, &mut self.state, &env.data_dir()),
                    );
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
