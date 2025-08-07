use crate::plugin::host::{PluginHandle, PluginHost};
use egui::{Margin, TextWrapMode};
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
    pub fn ui(&self, sync: &mut PluginHost, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for plugin in sync.plugins_mut() {
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
}
