use crate::sync::db::work::{DbWork, WorkId};
use crate::{
    shared::update::DataUpdate,
    sync::db::plugin::{DbPlugin, PluginId},
};
use anyhow::Result;
use artchiver_sdk::{PluginMetadata, Work};
use crossbeam::channel::{self, Receiver, Sender};
use log::{Level, debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default)]
pub enum Progress {
    #[default]
    None,
    Spinner,
    Percent {
        current: usize,
        total: usize,
    },
}

impl Progress {
    pub fn ui(&self, ui: &mut egui::Ui) {
        match self {
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
    }
}

// Hosts the main/ux process's singular message receiver. Other systems hand out wrappers
// around this system to send progress back to the main thread for display.
pub struct ProgressMonitor {
    tx_to_monitor: Sender<DataUpdate>,
    rx_from_all: Receiver<DataUpdate>,
}

impl Default for ProgressMonitor {
    fn default() -> Self {
        let (tx_to_monitor, rx_from_all) = channel::unbounded();
        Self {
            tx_to_monitor,
            rx_from_all,
        }
    }
}

impl ProgressMonitor {
    pub fn read(&mut self) -> Vec<DataUpdate> {
        let mut out = Vec::new();
        while let Ok(update) = self.rx_from_all.try_recv() {
            out.push(update);
        }
        out
    }

    pub fn monitor_channel(&self) -> Sender<DataUpdate> {
        self.tx_to_monitor.clone()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum UpdateSource {
    #[default]
    Unknown,
    Plugin(PluginId),
    DbWriter,
    DbReader,
}

#[derive(Clone, Debug)]
pub struct HostUpdateSender {
    pub source: UpdateSource,
    pub tx_to_runner: Sender<DataUpdate>,
}

impl HostUpdateSender {
    pub fn wrap(source: UpdateSource, tx_to_runner: Sender<DataUpdate>) -> Self {
        Self {
            source,
            tx_to_runner,
        }
    }

    pub fn channel(&self) -> Sender<DataUpdate> {
        self.tx_to_runner.clone()
    }

    pub fn plugin_loaded(
        &self,
        source: &Path,
        record: &DbPlugin,
        metadata: &PluginMetadata,
    ) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "plugin loaded before init"
        );
        self.tx_to_runner.send(DataUpdate::PluginInfo {
            source: source.to_owned(),
            record: record.clone(),
            metadata: metadata.clone(),
        })?;
        Ok(())
    }

    pub fn note_completed_task(&mut self) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "task completed before init"
        );
        self.tx_to_runner.send(DataUpdate::CompletedTask {
            source: self.source,
        })?;
        Ok(())
    }

    pub fn note_tags_were_refreshed(&mut self) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "tags refreshed before init"
        );
        self.tx_to_runner.send(DataUpdate::TagsWereRefreshed)?;
        Ok(())
    }

    pub fn note_works_were_refreshed(&mut self, for_tag: String) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "tags refreshed before init"
        );
        self.tx_to_runner
            .send(DataUpdate::WorksWereUpdatedForTag { for_tag })?;
        Ok(())
    }

    pub fn note_completed_download(
        &mut self,
        id: WorkId,
        preview_path: &str,
        screen_path: &str,
        archive_path: Option<&str>,
    ) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "task completed before init"
        );
        self.tx_to_runner.send(DataUpdate::WorkDownloadCompleted {
            id,
            preview_path: preview_path.to_owned(),
            screen_path: screen_path.to_owned(),
            archive_path: archive_path.map(|s| s.to_owned()),
        })?;
        Ok(())
    }
    
    pub fn fetch_works_completed(
        &mut self,
        works: HashMap<WorkId, DbWork>,
    ) -> Result<()> {
        assert_ne!(
            self.source,
            UpdateSource::Unknown,
            "task completed before init"
        );
        self.tx_to_runner.send(DataUpdate::FetchWorksComplete { works })?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct LogSender {
    pub source: UpdateSource,
    pub tx_to_runner: Sender<DataUpdate>,
}

impl LogSender {
    pub fn wrap(source: UpdateSource, tx_to_runner: Sender<DataUpdate>) -> Self {
        Self {
            source,
            tx_to_runner,
        }
    }

    pub fn channel(&self) -> Sender<DataUpdate> {
        self.tx_to_runner.clone()
    }

    pub fn trace<S: fmt::Display>(&mut self, message: S) {
        trace!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log {
                source: self.source,
                level: Level::Trace,
                message: message.to_string(),
            })
            .ok();
    }

    pub fn debug<S: fmt::Display>(&mut self, message: S) {
        debug!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log {
                source: self.source,
                level: Level::Debug,
                message: message.to_string(),
            })
            .ok();
    }

    pub fn info<S: fmt::Display>(&mut self, message: S) {
        info!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log {
                source: self.source,
                level: Level::Info,
                message: message.to_string(),
            })
            .ok();
    }

    pub fn warn<S: fmt::Display>(&mut self, message: S) {
        warn!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log {
                source: self.source,
                level: Level::Warn,
                message: message.to_string(),
            })
            .ok();
    }

    pub fn error<S: fmt::Display>(&mut self, message: S) {
        error!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log {
                source: self.source,
                level: Level::Error,
                message: message.to_string(),
            })
            .ok();
    }

    pub fn log_message<S: fmt::Display>(&mut self, level: u32, message: S) {
        match level {
            0 => self.trace(message),
            1 => self.debug(message),
            2 => self.info(message),
            3 => self.warn(message),
            _ => self.error(message),
        };
    }
}

#[derive(Clone, Debug)]
pub struct ProgressSender {
    source: UpdateSource,
    tx_to_runner: Sender<DataUpdate>,
}

impl ProgressSender {
    pub fn wrap(source: UpdateSource, tx_to_runner: Sender<DataUpdate>) -> Self {
        Self {
            source,
            tx_to_runner,
        }
    }

    pub fn channel(&self) -> Sender<DataUpdate> {
        self.tx_to_runner.clone()
    }

    pub fn clear(&mut self) {
        self.tx_to_runner
            .send(DataUpdate::Progress {
                source: self.source,
                progress: Progress::None,
            })
            .ok();
    }

    pub fn set_spinner(&mut self) {
        self.tx_to_runner
            .send(DataUpdate::Progress {
                source: self.source,
                progress: Progress::Spinner,
            })
            .ok();
    }

    pub fn set_percent(&mut self, current: usize, total: usize) {
        self.tx_to_runner
            .send(DataUpdate::Progress {
                source: self.source,
                progress: Progress::Percent { current, total },
            })
            .ok();
    }
}
