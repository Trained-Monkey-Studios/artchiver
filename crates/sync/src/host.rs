use crate::{
    caching_pool::CachingPool,
    environment::Environment,
    model::{MetadataPool, MetadataPoolResource},
    plugin::load_plugin,
    progress::Progress,
    shared::{PluginMetadata, PluginRequest, PluginResponse},
};
use bevy::{
    prelude::*,
    tasks::{Task, block_on},
};
use crossbeam::channel;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
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

pub(crate) fn startup_plugins(
    metadata: Res<MetadataPoolResource>,
    env: Res<Environment>,
    mut commands: Commands,
) -> Result {
    let mut engine = PluginHost::new(metadata.pool());

    let mut all_wasm = search_for_plugins_to_load(&env)?;
    for source in all_wasm.drain(..) {
        let (tx_to_plugin, rx_from_runner) = channel::unbounded();
        let (tx_to_runner, rx_from_plugin) = channel::unbounded();

        let plugin_task = load_plugin(
            &source,
            &env.cache_dir(),
            metadata.pool(),
            rx_from_runner,
            tx_to_runner,
        )?;
        engine.plugins.push(PluginHandle {
            source,
            metadata: None,
            progress: Progress::None,
            messages: VecDeque::new(),
            task: plugin_task,
            tx_to_plugin,
            rx_from_plugin,
        });
    }

    commands.insert_resource(engine);
    Ok(())
}

// The PluginHost holds all of the PluginHandles -- the bevy side of the plugin state -- and
// aggregates across our plugins to provide a unified data layer.
#[derive(Debug, Resource)]
pub struct PluginHost {
    plugins: Vec<PluginHandle>,
    pool: CachingPool,
}

impl PluginHost {
    fn new(pool: MetadataPool) -> Self {
        Self {
            plugins: Vec::new(),
            pool: CachingPool::new(pool),
        }
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

    pub fn refresh_tags(&self) -> Result {
        for plugin in self.plugins.iter() {
            plugin.tx_to_plugin.send(PluginRequest::RefreshTags)?;
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

    // Maintenance state
    task: Task<()>,
    tx_to_plugin: channel::Sender<PluginRequest>,
    rx_from_plugin: channel::Receiver<PluginResponse>,
}

impl PluginHandle {
    const MAX_MESSAGES: usize = 20;

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn metadata(&self) -> Option<&PluginMetadata> {
        self.metadata.as_ref()
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

    pub fn progress(&self) -> &Progress {
        &self.progress
    }
}

pub(crate) fn maintain_plugins(
    mut engine: ResMut<PluginHost>,
    app_exit: EventReader<AppExit>,
) -> Result {
    let mut database_changed = false;
    for handle in engine.plugins.iter_mut() {
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
        engine.pool.bump_generation();
    }

    // Proxy shutdown to our plugins and wait for them to terminate.
    if !app_exit.is_empty() {
        for handle in engine.plugins.drain(..) {
            handle.tx_to_plugin.send(PluginRequest::Shutdown)?;
            block_on(handle.task);
        }
    }

    Ok(())
}
