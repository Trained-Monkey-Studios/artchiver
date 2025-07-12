use crate::plugin::PluginResponse;
use crossbeam::channel::Sender;

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    None,
    Spinner,
    Percent { current: usize, total: usize },
}

pub struct ProgressSender {
    tx_to_runner: Sender<PluginResponse>,
}

impl ProgressSender {
    pub(crate) fn wrap(tx_to_runner: Sender<PluginResponse>) -> Self {
        Self { tx_to_runner }
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
        self.tx_to_runner
            .send(PluginResponse::Message(message.as_ref().to_string()))
            .ok();
    }
}
