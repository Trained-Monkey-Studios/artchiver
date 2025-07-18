use crate::{
    shared::{
        environment::Environment,
        plugin::{PluginRequest, PluginResponse},
        progress::Progress,
    },
    sync::{
        db::{caching_pool::CachingPool, model::MetadataPool},
        plugin::client::create_plugin_task,
    },
};
use anyhow::Result;
use artchiver_sdk::PluginMetadata;
use crossbeam::channel;
use log::Level;
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
#[derive(Debug)]
pub struct PluginHost {
    plugins: Vec<PluginHandle>,
    pool: CachingPool,
}

impl PluginHost {
    pub fn new(pool: MetadataPool, env: &Environment) -> Result<Self> {
        let mut plugins = Vec::new();
        for source in search_for_plugins_to_load(env)?.drain(..) {
            let (tx_to_plugin, rx_from_runner) = channel::unbounded();
            let (tx_to_runner, rx_from_plugin) = channel::unbounded();

            let plugin_task =
                create_plugin_task(&source, env, pool.clone(), rx_from_runner, tx_to_runner)?;
            plugins.push(PluginHandle::new(
                source,
                plugin_task,
                tx_to_plugin,
                rx_from_plugin,
            ));
        }
        Ok(Self {
            plugins,
            pool: CachingPool::new(pool),
        })
    }

    pub fn pool(&self) -> &CachingPool {
        &self.pool
    }

    pub fn pool_mut(&mut self) -> &mut CachingPool {
        &mut self.pool
    }

    pub fn plugins(&self) -> impl Iterator<Item = &PluginHandle> {
        self.plugins.iter()
    }

    pub fn plugins_mut(&mut self) -> impl Iterator<Item = &mut PluginHandle> {
        self.plugins.iter_mut()
    }

    pub fn refresh_works_for_tag(&mut self, tag: &str) -> Result<()> {
        let plugin_names = self.pool.list_plugins_for_tag(tag)?;
        for plugin in &mut self.plugins {
            // Only ask for matching works if the tag came from a plugin.
            if plugin_names.contains(&plugin.name()) {
                plugin
                    .task_queue
                    .push_back(PluginRequest::RefreshWorksForTag {
                        tag: tag.to_owned(),
                    });
            }
        }
        Ok(())
    }

    pub(crate) fn maintain_plugins(&mut self) {
        // Receive messages from plugin
        let mut database_changed = false;
        for plugin in &mut self.plugins {
            while let Ok(msg) = plugin.rx_from_plugin.try_recv() {
                match msg {
                    PluginResponse::Progress(progress) => {
                        plugin.progress = progress;
                    }
                    PluginResponse::Log(level, message) => {
                        plugin.log_messages.push_front((level, message));
                        while plugin.log_messages.len() > PluginHandle::MAX_MESSAGES {
                            plugin.log_messages.pop_back();
                        }
                    }
                    PluginResponse::PluginInfo(info) => {
                        plugin.metadata = Some(info);
                    }
                    PluginResponse::DatabaseChanged => {
                        database_changed = true;
                    }
                    PluginResponse::CompletedTask => {
                        plugin.active_task = None;
                    }
                }
            }
            if plugin.active_task.is_none() {
                if let Some(task) = plugin.task_queue.pop_front() {
                    plugin.active_task = Some(task.clone());
                    plugin.tx_to_plugin.send(task).ok();
                }
            }
        }
        if database_changed {
            self.pool.bump_generation();
        }
    }

    pub fn cleanup_for_exit(&mut self) -> Result<()> {
        for handle in self.plugins.drain(..) {
            handle.tx_to_plugin.send(PluginRequest::Shutdown)?;
            handle.task.join().ok();
        }
        Ok(())
    }
}

// The PluginHandle lives in the bevy environment.
#[derive(Debug)]
pub struct PluginHandle {
    // Metadata
    source: PathBuf,
    metadata: Option<PluginMetadata>,

    // Active state
    progress: Progress,
    log_messages: VecDeque<(Level, String)>,
    active_task: Option<PluginRequest>,
    task_queue: VecDeque<PluginRequest>,

    // Maintenance state
    task: JoinHandle<()>,
    tx_to_plugin: channel::Sender<PluginRequest>,
    rx_from_plugin: channel::Receiver<PluginResponse>,
}

impl PluginHandle {
    const MAX_MESSAGES: usize = 50;

    fn new(
        source: PathBuf,
        task: JoinHandle<()>,
        tx_to_plugin: channel::Sender<PluginRequest>,
        rx_from_plugin: channel::Receiver<PluginResponse>,
    ) -> Self {
        Self {
            source,
            metadata: None,
            progress: Progress::None,
            log_messages: VecDeque::new(),
            active_task: None,
            task_queue: VecDeque::new(),
            task,
            tx_to_plugin,
            rx_from_plugin,
        }
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

    pub fn remove_queued_task(&mut self, index: usize) {
        self.task_queue.remove(index);
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
        self.tx_to_plugin.send(PluginRequest::ApplyConfiguration {
            config: self
                .metadata
                .as_ref()
                .map(|v| v.configurations().to_vec())
                .unwrap_or_default(),
        })?;
        Ok(())
    }
}
