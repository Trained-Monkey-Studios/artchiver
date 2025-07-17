use crate::shared::plugin::PluginResponse;
use anyhow::Result;
use crossbeam::channel::Sender;
use log::{info, trace};

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

    pub fn message(&mut self, message: impl AsRef<str>) {
        let msg = message.as_ref();
        info!("{msg}");
        self.tx_to_runner
            .send(PluginResponse::Message(msg.to_owned()))
            .ok();
        self.tx_to_runner
            .send(PluginResponse::Trace(msg.to_owned()))
            .ok();
    }

    pub fn trace(&mut self, message: impl AsRef<str>) {
        let msg = message.as_ref();
        trace!("{msg}");
        self.tx_to_runner
            .send(PluginResponse::Trace(msg.to_owned()))
            .ok();
    }
}
