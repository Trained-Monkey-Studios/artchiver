use crate::shared::{
    progress::{Progress, UpdateSource},
    update::DataUpdate,
};
use log::log;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UxDb {
    messages: VecDeque<String>,

    #[serde(skip)]
    progress: Progress,
}

impl UxDb {
    const MAX_MESSAGES: usize = 20;

    pub fn handle_updates(&mut self, updates: &[DataUpdate]) {
        for update in updates {
            match update {
                DataUpdate::Log {
                    source,
                    level,
                    message,
                } if source == &UpdateSource::DbWriter => {
                    log!(*level, "{message}");
                    self.messages.push_front(message.to_owned());
                    while self.messages.len() > Self::MAX_MESSAGES {
                        self.messages.pop_back();
                    }
                }
                DataUpdate::Progress { source, progress } if source == &UpdateSource::DbWriter => {
                    self.progress = *progress;
                }
                _ => {}
            }
        }
    }

    pub fn ui(&self, ui: &mut egui::Ui) {
        self.progress.ui(ui);
        for message in &self.messages {
            ui.label(message);
        }
    }
}
