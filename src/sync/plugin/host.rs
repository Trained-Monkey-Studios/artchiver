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
        for source in search_for_plugins_to_load(&env)?.drain(..) {
            let (tx_to_plugin, rx_from_runner) = channel::unbounded();
            let (tx_to_runner, rx_from_plugin) = channel::unbounded();

            let plugin_task =
                create_plugin_task(&source, &env, pool.clone(), rx_from_runner, tx_to_runner)?;
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

    pub fn refresh_tags(&self) -> Result<()> {
        for plugin in self.plugins.iter() {
            plugin.tx_to_plugin.send(PluginRequest::RefreshTags)?;
        }
        Ok(())
    }

    pub fn refresh_works_for_tag(&mut self, tag: &str) -> Result<()> {
        let plugin_names = self.pool.list_plugins_for_tag(tag)?;
        for plugin in &self.plugins {
            // Only ask for matching works if the tag came from a plugin.
            if plugin_names.contains(&plugin.name()) {
                plugin
                    .tx_to_plugin
                    .send(PluginRequest::RefreshWorksForTag {
                        tag: tag.to_owned(),
                    })?;
            }
        }
        Ok(())
    }

    pub(crate) fn maintain_plugins(&mut self) {
        let mut database_changed = false;
        for handle in self.plugins.iter_mut() {
            while let Ok(msg) = handle.rx_from_plugin.try_recv() {
                match msg {
                    PluginResponse::Progress(progress) => {
                        handle.progress = progress;
                    }
                    PluginResponse::Message(message) => {
                        handle.messages.push_back(message);
                        while handle.messages.len() > PluginHandle::MAX_MESSAGES {
                            handle.messages.pop_front();
                        }
                    }
                    PluginResponse::Trace(message) => {
                        handle.traces.push_back(message);
                        while handle.traces.len() > PluginHandle::MAX_TRACES {
                            handle.traces.pop_front();
                        }
                    }
                    PluginResponse::PluginInfo(info) => {
                        handle.metadata = Some(info);
                    }
                    PluginResponse::DatabaseChanged => {
                        database_changed = true;
                    }
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
            let _ = handle.task.join();
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
    progress: Progress,
    messages: VecDeque<String>,
    traces: VecDeque<String>,

    // Maintenance state
    task: JoinHandle<()>,
    tx_to_plugin: channel::Sender<PluginRequest>,
    rx_from_plugin: channel::Receiver<PluginResponse>,
}

impl PluginHandle {
    const MAX_MESSAGES: usize = 20;
    const MAX_TRACES: usize = 100;

    fn new(
        source: PathBuf,
        task: JoinHandle<()>,
        tx_to_plugin: channel::Sender<PluginRequest>,
        rx_from_plugin: channel::Receiver<PluginResponse>,
    ) -> Self {
        PluginHandle {
            source,
            metadata: None,
            progress: Progress::None,
            messages: VecDeque::new(),
            traces: VecDeque::new(),
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

    pub fn messages(&self) -> impl Iterator<Item = &str> {
        self.messages.iter().map(|s| s.as_str())
    }

    pub fn traces(&self) -> impl Iterator<Item = &str> {
        self.traces.iter().map(|s| s.as_str())
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn apply_configuration(&self) -> Result<()> {
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
