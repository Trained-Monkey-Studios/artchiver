use crate::{
    plugin::host::{PluginHandle, PluginHost},
    ux::tutorial::{NextButton, Tutorial, TutorialStep},
};
use artchiver_sdk::ConfigValue;
use egui::{Margin, TextWrapMode};
use egui_dnd::{DragUpdate, dnd};
use log::Level;
use serde::{Deserialize, Serialize};

// Utility function to get an egui margin inset from the left.
fn indented(px: i8) -> Margin {
    let mut m = Margin::ZERO;
    m.left = px;
    m
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UxPlugin {}

impl UxPlugin {
    pub fn ui(&self, sync: &mut PluginHost, mut tutorial: Tutorial<'_>, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if tutorial.step() == TutorialStep::PluginsIntro {
                    tutorial.frame(ui, |ui, tutorial| {
                        ui.heading("About Plugins").scroll_to_me(None);
                        ui.separator();
                        ui.label("Plugins provide access to data sources, like the National Gallery of Art or the Met's open-access collections.");
                        ui.label("");
                        ui.label("You can add new data sources by dropping a plugin for that data source into the plugins directory and restarting.");
                        ui.label("");
                        ui.label("New plugins are easy to build if a collection you want to access is not already supported. See the documentation to get started.");
                        tutorial.button_area(NextButton::Next, ui);
                    });
                }

                for plugin in sync.plugins_mut() {
                    let name = plugin.name();
                    if tutorial.is_plugin_refresh_step(&name) {
                        tutorial.frame(ui, |ui, tutorial| {
                            ui.heading("Fetching Tags").scroll_to_me(None);
                            ui.separator();
                            ui.label("First Step: Click on a plugin's \"⟳ Tags\" button to fetch or refresh the tags that plugin knows about. Do so now for the National Gallery of Art.");
                            tutorial.button_area(NextButton::Skip, ui);
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.heading(&name);

                        if tutorial.add(tutorial.is_plugin_refresh_step(&name), ui, egui::Button::new("⟳ Tags")).clicked() {
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
                    ui.separator();
                }
            });
    }

    fn show_plugin_details(ui: &mut egui::Ui, plugin: &mut PluginHandle) {
        egui::CollapsingHeader::new("Details")
            .id_salt(format!("details_section_{}", plugin.name()))
            .show(ui, |ui| -> anyhow::Result<()> {
                ui.label(plugin.description());
                egui::Grid::new(format!("plugin_grid_{}", plugin.name()))
                    .num_columns(2)
                    .show(ui, |ui| -> anyhow::Result<()> {
                        ui.label("Source");
                        ui.label(plugin.source().display().to_string());
                        ui.end_row();
                        ui.label("Version");
                        ui.label(plugin.version());
                        ui.end_row();
                        if let Some(meta) = plugin.metadata_mut() {
                            for (config_key, config_val) in meta.configurations_mut() {
                                match config_val {
                                    ConfigValue::String(s) => {
                                        ui.label(config_key);
                                        ui.text_edit_singleline(s);
                                        ui.end_row();
                                    }
                                    ConfigValue::StringList(v) => {
                                        ui.label(config_key);
                                        if ui.button("Add Item").clicked() {
                                            v.push(String::new());
                                        }
                                        ui.end_row();

                                        for (i, s) in v.iter_mut().enumerate() {
                                            ui.label(format!("Item {i}"));
                                            ui.text_edit_singleline(s);
                                            ui.end_row();
                                        }
                                    }
                                }
                            }
                            if !meta.configurations().is_empty() {
                                ui.label("");
                                if ui.button("Update").clicked() {
                                    plugin.apply_configuration()?;
                                }
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
                let mut clear_all = false;
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
                            if plugin.task_queue_len() > 0
                                && ui.small_button("x Cancel All").clicked()
                            {
                                plugin.cancellation().cancel();
                                clear_all = true;
                            }
                        });
                    }
                    None => {
                        ui.label("Inactive");
                    }
                }
                if clear_all {
                    plugin.clear_queued_tasks();
                }
                ui.separator();

                let mut offset = 0;
                let mut removed = None;
                let resp = dnd(ui, format!("task_queue_{}", plugin.name())).show(
                    plugin.task_queue(),
                    |ui, req, handle, _state| {
                        handle.ui(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button("x").on_hover_text("Cancel").clicked() {
                                    removed = Some(req.clone());
                                }
                                ui.label(format!("{offset}: {req}"));
                                offset += 1;
                            });
                        });
                    },
                );
                if let Some(req) = removed {
                    plugin.remove_queued_task(&req);
                } else if let Some(DragUpdate { from, to }) = resp.update {
                    plugin.swap_task_queue_items(from, to);
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
}
