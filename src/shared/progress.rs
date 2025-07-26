use crate::shared::update::DataUpdate;
use anyhow::Result;
use crossbeam::channel::{self, Receiver, Sender};
use log::{Level, debug, error, info, trace, warn};
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    None,
    Spinner,
    Percent { current: usize, total: usize },
}

// Hosts the main/ux process's singular message receiver. Other systems hand out wrappers
// around this system to send progress back to the main thread for display.
pub struct ProgressMonitor {
    tx_to_monitor: Sender<DataUpdate>,
    rx_from_all: Receiver<DataUpdate>,
}

impl ProgressMonitor {
    pub fn new() -> Self {
        let (tx_to_monitor, rx_from_all) = channel::unbounded();
        Self {
            tx_to_monitor,
            rx_from_all,
        }
    }

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

#[derive(Clone, Debug)]
pub struct ProgressSender {
    tx_to_runner: Sender<DataUpdate>,
}

impl ProgressSender {
    pub(crate) fn wrap(tx_to_runner: Sender<DataUpdate>) -> Self {
        Self { tx_to_runner }
    }

    
    // FIXME: Reconsider these APIS
    pub fn send(&mut self, message: DataUpdate) -> Result<()> {
        self.tx_to_runner.send(message)?;
        Ok(())
    }

    pub fn note_tags_refreshed(&mut self) -> Result<()> {
        println!("ABOUT TO SEND TO tx_to_runner");
        self.tx_to_runner.send(DataUpdate::TagsRefreshed)?;
        Ok(())
    }

    pub fn note_completed_task(&mut self) -> Result<()> {
        self.tx_to_runner.send(DataUpdate::CompletedTask)?;
        Ok(())
    }
    // ^^
    ////////////////////////////

    pub fn clear(&mut self) {
        self.tx_to_runner
            .send(DataUpdate::Progress(Progress::None))
            .ok();
    }

    pub fn set_spinner(&mut self) {
        self.tx_to_runner
            .send(DataUpdate::Progress(Progress::Spinner))
            .ok();
    }

    pub fn set_percent(&mut self, current: usize, total: usize) {
        self.tx_to_runner
            .send(DataUpdate::Progress(Progress::Percent { current, total }))
            .ok();
    }

    pub fn trace<S: fmt::Display>(&mut self, message: S) {
        trace!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log(Level::Trace, message.to_string()))
            .ok();
    }

    pub fn debug<S: fmt::Display>(&mut self, message: S) {
        debug!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log(Level::Debug, message.to_string()))
            .ok();
    }

    pub fn info<S: fmt::Display>(&mut self, message: S) {
        info!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log(Level::Info, message.to_string()))
            .ok();
    }

    pub fn warn<S: fmt::Display>(&mut self, message: S) {
        warn!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log(Level::Warn, message.to_string()))
            .ok();
    }

    pub fn error<S: fmt::Display>(&mut self, message: S) {
        error!("{message}");
        self.tx_to_runner
            .send(DataUpdate::Log(Level::Error, message.to_string()))
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
