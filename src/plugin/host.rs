use crate::{
    db::{
        models::{
            plugin::{DbPlugin, PluginId},
            tag::DbTag,
        },
        sync::DbSyncHandle,
        writer::DbWriteHandle,
    },
    plugin::client::create_plugin_task,
    shared::{
        environment::Environment,
        plugin::{PluginCancellation, PluginRequest},
        progress::{Progress, ProgressMonitor, UpdateSource},
        update::DataUpdate,
    },
};
use anyhow::Result;
use artchiver_sdk::PluginMetadata;
use crossbeam::channel;
use log::{Level, error};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    thread::JoinHandle,
};

fn search_for_plugins_to_load(env: &Environment) -> Result<Vec<PathBuf>> {
    let mut rv = Vec::new();
    for dir in &[env.global_plugin_dir(), env.local_plugin_dir()] {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let is_wasm_ext = entry
                .path()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".wasm");
            if is_wasm_ext && (entry.path().is_file() || entry.path().is_symlink()) {
                rv.push(entry.path());
            }
        }
    }
    Ok(rv)
}

// The PluginHost holds all PluginHandles -- the egui side of the plugin state -- and
// aggregates across our plugins to provide a unified data layer.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PluginHost {
    plugins: Vec<PluginHandle>,

    #[serde(skip)]
    db: Option<DbSyncHandle>,
}

impl PluginHost {
    // We want to keep our plugin queues between runs, but serde deserialize needs us to
    // default-initialize. So we do that, then re-build from that state with our live handles.
    //
    // The hard part here is making sure our restored environment broadly matches the plugins
    // directory. This is why artchiver stores the saved state in the data dir, rather than
    // in a the user's dotfiles. Regardless, plugins need a complete from-scratch initialization
    // every time, even if we already have queued tasks etc.
    pub fn initialize(
        &mut self,
        env: &Environment,
        progress_mon: &ProgressMonitor,
        db_sync: &DbSyncHandle,
        db_write: &DbWriteHandle,
    ) -> Result<()> {
        for source in search_for_plugins_to_load(env)?.drain(..) {
            let (tx_to_plugin, rx_from_runner) = channel::unbounded();

            match create_plugin_task(
                &source,
                env,
                db_sync.clone(),
                db_write.clone(),
                rx_from_runner,
                progress_mon.monitor_channel(),
            ) {
                Ok((plugin_task, cancellation)) => {
                    if let Some(plugin) = self.plugins.iter_mut().find(|p| p.source() == source) {
                        plugin.initialize(&source, plugin_task, cancellation, tx_to_plugin);
                    } else {
                        let mut plugin = PluginHandle::default();
                        plugin.initialize(&source, plugin_task, cancellation, tx_to_plugin);
                        self.plugins.push(plugin);
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to load plugin {}: {}", source.display(), e);
                    error!("{msg}");
                    progress_mon.monitor_channel().send(DataUpdate::Log {
                        source: UpdateSource::Unknown,
                        level: Level::Error,
                        message: msg,
                    })?;
                }
            }
        }
        // Remove any that we didn't find via search_for_plugins_to_load.
        self.plugins.retain(|p| p.remote.is_some());

        self.db = Some(db_sync.clone());
        Ok(())
    }

    pub fn plugins(&self) -> impl Iterator<Item = &PluginHandle> {
        self.plugins.iter()
    }

    pub fn plugins_mut(&mut self) -> impl Iterator<Item = &mut PluginHandle> {
        self.plugins.iter_mut()
    }

    pub fn refresh_works_for_tag(&mut self, tag: &DbTag) -> Result<()> {
        let plugin_ids = self
            .db
            .as_ref()
            .expect("uninit")
            .sync_list_plugins_for_tag(tag.id())?;
        for plugin in &mut self.plugins {
            if let Some(plugin_id) = plugin.id() {
                // Only ask for matching works if the tag came from a plugin.
                if plugin_ids.contains(&plugin_id) {
                    plugin
                        .task_queue
                        .push_back(PluginRequest::RefreshWorksForTag {
                            tag: tag.name().to_owned(),
                        });
                }
            }
        }
        Ok(())
    }

    pub fn handle_updates(&mut self, updates: &[DataUpdate]) {
        for plugin in &mut self.plugins {
            plugin.handle_updates(updates);
        }
    }

    pub fn cleanup_for_exit(&mut self) -> Result<()> {
        for plugin in self.plugins.drain(..) {
            plugin.cleanup_for_exit()?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct PluginRemote {
    task: JoinHandle<()>,
    cancellation: PluginCancellation,
    tx_to_plugin: channel::Sender<PluginRequest>,
}

impl PluginRemote {
    fn new(
        task: JoinHandle<()>,
        cancellation: PluginCancellation,
        tx_to_plugin: channel::Sender<PluginRequest>,
    ) -> Self {
        Self {
            task,
            cancellation,
            tx_to_plugin,
        }
    }

    fn cleanup_for_exit(self) -> Result<()> {
        self.cancellation.cancel();
        self.tx_to_plugin.send(PluginRequest::Shutdown)?;
        self.task.join().ok();
        Ok(())
    }
}

// The PluginHandle lives in the bevy environment.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PluginHandle {
    // Metadata
    source: PathBuf,
    metadata: Option<PluginMetadata>,
    record: Option<DbPlugin>,

    // Transient state that is lost across runs
    #[serde(skip)]
    progress: Progress,
    #[serde(skip)]
    log_messages: VecDeque<(Level, String)>,

    // Persistent state that is saved between runs
    active_task: Option<PluginRequest>,
    task_queue: VecDeque<PluginRequest>,

    // Maintenance state
    #[serde(skip)]
    remote: Option<PluginRemote>,
}

impl PluginHandle {
    const MAX_MESSAGES: usize = 50;

    fn initialize(
        &mut self,
        source: &Path,
        task: JoinHandle<()>,
        cancellation: PluginCancellation,
        tx_to_plugin: channel::Sender<PluginRequest>,
    ) {
        assert!(self.remote.is_none(), "reinitializing a plugin");
        self.source = source.to_owned();
        self.remote = Some(PluginRemote::new(task, cancellation, tx_to_plugin));
    }

    fn cleanup_for_exit(mut self) -> Result<()> {
        self.remote
            .take()
            .expect("cleaning up a uninit plugin")
            .cleanup_for_exit()
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn metadata(&self) -> Option<&PluginMetadata> {
        self.metadata.as_ref()
    }

    pub fn metadata_mut(&mut self) -> Option<&mut PluginMetadata> {
        self.metadata.as_mut()
    }

    pub fn name(&self) -> String {
        if let Some(metadata) = self.metadata.as_ref() {
            metadata.name().to_owned()
        } else {
            self.source().display().to_string()
        }
    }

    pub fn id(&self) -> Option<PluginId> {
        self.record.as_ref().map(|v| v.id())
    }

    pub fn description(&self) -> String {
        if let Some(metadata) = self.metadata.as_ref() {
            metadata.description().to_owned()
        } else {
            "not yet loaded".to_owned()
        }
    }

    pub fn version(&self) -> String {
        if let Some(metadata) = self.metadata.as_ref() {
            metadata.version().to_owned()
        } else {
            "not yet loaded".to_owned()
        }
    }

    pub fn log_messages(&self) -> impl Iterator<Item = (Level, &str)> {
        self.log_messages.iter().map(|(lvl, s)| (*lvl, s.as_str()))
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn active_task(&self) -> Option<&PluginRequest> {
        self.active_task.as_ref()
    }

    pub fn task_queue(&self) -> impl Iterator<Item = &PluginRequest> {
        self.task_queue.iter()
    }

    pub fn task_queue_len(&self) -> usize {
        self.task_queue.len()
    }

    pub fn swap_task_queue_items(&mut self, from: usize, to: usize) {
        if (0..self.task_queue.len()).contains(&from) && (0..self.task_queue.len()).contains(&to) {
            self.task_queue.swap(from, to);
        }
    }

    pub fn remove_queued_task(&mut self, req: &PluginRequest) {
        self.task_queue.retain(|v| v != req);
    }

    pub fn clear_queued_tasks(&mut self) {
        self.task_queue.clear();
    }

    pub fn cancellation(&self) -> &PluginCancellation {
        &self
            .remote
            .as_ref()
            .expect("uninitialized plugin")
            .cancellation
    }

    pub fn refresh_tags(&mut self) {
        self.task_queue.push_back(PluginRequest::RefreshTags);
    }

    pub fn apply_configuration(&self) -> Result<()> {
        // Note: we short cut the queue here, as config needs to apply immediately.
        //       This also doesn't send a return CompletedTask, so the CompletedTask
        //       of anything we're after will enqueue the next task after us. This
        //       will block a bit while the ApplyConfiguration runs, but this should
        //       be fast enough not to notice.
        self.remote.as_ref().expect("uninit").tx_to_plugin.send(
            PluginRequest::ApplyConfiguration {
                config: self
                    .metadata
                    .as_ref()
                    .map(|v| v.configurations().to_vec())
                    .unwrap_or_default(),
            },
        )?;
        Ok(())
    }

    pub fn handle_updates(&mut self, updates: &[DataUpdate]) {
        for update in updates {
            match update {
                DataUpdate::PluginInfo {
                    source,
                    record,
                    metadata,
                } if source == &self.source => {
                    self.metadata = Some(metadata.to_owned());
                    self.record = Some(record.to_owned());

                    // Note: only restart our restored active task once init is finished.
                    if let Some(req) = self.active_task.as_ref() {
                        self.remote
                            .as_ref()
                            .expect("uninit")
                            .tx_to_plugin
                            .send(req.clone())
                            .expect("sent to stopped plugin");
                    }
                }
                DataUpdate::Log {
                    source: UpdateSource::Plugin(id),
                    level,
                    message,
                } if Some(*id) == self.id() => {
                    self.log_messages.push_front((*level, message.to_owned()));
                    while self.log_messages.len() > Self::MAX_MESSAGES {
                        self.log_messages.pop_back();
                    }
                }
                DataUpdate::Progress {
                    source: UpdateSource::Plugin(id),
                    progress,
                } if Some(*id) == self.id() => {
                    self.progress = *progress;
                }
                DataUpdate::CompletedTask {
                    source: UpdateSource::Plugin(id),
                } if Some(*id) == self.id() => {
                    self.active_task = None;
                }
                _ => {}
            }
        }
        if self.active_task.is_none() {
            if let Some(task) = self.task_queue.pop_front() {
                self.active_task = Some(task.clone());
                self.remote
                    .as_ref()
                    .expect("uninit")
                    .tx_to_plugin
                    .send(task)
                    .expect("sent to stopped plugin");
            }
        }
    }
}
