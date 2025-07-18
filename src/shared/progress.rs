use crate::shared::plugin::PluginResponse;
use anyhow::Result;
use crossbeam::channel::Sender;
use log::{Level, debug, error, info, trace, warn};
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    None,
    Spinner,
    Percent { current: usize, total: usize },
}

#[derive(Clone, Debug)]
pub struct ProgressSender {
    tx_to_runner: Sender<PluginResponse>,
}

impl ProgressSender {
    pub(crate) fn wrap(tx_to_runner: Sender<PluginResponse>) -> Self {
        Self { tx_to_runner }
    }

    pub fn send(&mut self, message: PluginResponse) -> Result<()> {
        self.tx_to_runner.send(message)?;
        Ok(())
    }

    pub fn database_changed(&mut self) -> Result<()> {
        self.tx_to_runner.send(PluginResponse::DatabaseChanged)?;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.tx_to_runner
            .send(PluginResponse::Progress(Progress::None))
            .ok();
    }

    pub fn set_spinner(&mut self) {
        self.tx_to_runner
            .send(PluginResponse::Progress(Progress::Spinner))
            .ok();
    }

    pub fn set_percent(&mut self, current: usize, total: usize) {
        self.tx_to_runner
            .send(PluginResponse::Progress(Progress::Percent {
                current,
                total,
            }))
            .ok();
    }

    pub fn trace<S: fmt::Display>(&mut self, message: S) {
        trace!("{message}");
        self.tx_to_runner
            .send(PluginResponse::Log(Level::Trace, message.to_string()))
            .ok();
    }

    pub fn debug<S: fmt::Display>(&mut self, message: S) {
        debug!("{message}");
        self.tx_to_runner
            .send(PluginResponse::Log(Level::Debug, message.to_string()))
            .ok();
    }

    pub fn info<S: fmt::Display>(&mut self, message: S) {
        info!("{message}");
        self.tx_to_runner
            .send(PluginResponse::Log(Level::Info, message.to_string()))
            .ok();
    }

    pub fn warn<S: fmt::Display>(&mut self, message: S) {
        warn!("{message}");
        self.tx_to_runner
            .send(PluginResponse::Log(Level::Warn, message.to_string()))
            .ok();
    }

    pub fn error<S: fmt::Display>(&mut self, message: S) {
        error!("{message}");
        self.tx_to_runner
            .send(PluginResponse::Log(Level::Error, message.to_string()))
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

    pub fn note_completed_task(&mut self) -> Result<()> {
        self.tx_to_runner.send(PluginResponse::CompletedTask)?;
        Ok(())
    }
}
