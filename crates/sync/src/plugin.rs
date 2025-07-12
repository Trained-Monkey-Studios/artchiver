use crate::{
    Environment,
    model::{MetadataPlugin, MetadataPool, MetadataPoolResource, MetadataSet},
    progress::{Progress, ProgressSender},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on},
};
use crossbeam::channel;
use extism::{
    Manifest, PTR, Plugin as ExtPlugin, PluginBuilder, UserData, Wasm, convert::Json, host_fn,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use ureq::Agent;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, SystemSet)]
pub enum SyncSet {
    StartupEngine,
    MaintainPlugins,
}

pub struct SyncPlugin;
impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MetadataPlugin)
            .add_systems(
                Startup,
                startup_sync_engine
                    .in_set(SyncSet::StartupEngine)
                    .after(MetadataSet::Connect),
            )
            .add_systems(
                FixedUpdate,
                maintain_plugins.in_set(SyncSet::MaintainPlugins),
            );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginMetadata {
    name: String,
    version: String,
    description: String,
}

impl PluginMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Clone, Debug)]
pub struct PluginState {
    cache_dir: PathBuf,
    pool: MetadataPool,
    agent: Agent,
}

impl PluginState {
    fn new(cache_dir: &Path, pool: MetadataPool) -> Self {
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        Self {
            cache_dir: cache_dir.to_owned(),
            pool,
            agent: Agent::new_with_config(
                Agent::config_builder()
                    .no_delay(true)
                    .http_status_as_error(true)
                    .user_agent(format!("Artchiver/{VERSION}"))
                    .max_response_header_size(256 * 1024)
                    .timeout_global(Some(Duration::from_secs(30)))
                    .timeout_recv_body(Some(Duration::from_secs(60)))
                    .build(),
            ),
        }
    }

    fn cache_dir(state: &UserData<PluginState>) -> PathBuf {
        state.get().unwrap().lock().unwrap().cache_dir.clone()
    }

    fn agent(state: &UserData<PluginState>) -> Agent {
        state.get().unwrap().lock().unwrap().agent.clone()
    }
}

#[derive(Clone, Debug)]
enum PluginRequest {
    Shutdown,
    RefreshTags,
}

#[derive(Clone, Debug)]
pub(crate) enum PluginResponse {
    PluginInfo(PluginMetadata),
    Progress(Progress),
    Message(String),
}

#[derive(Debug)]
pub struct PluginHandle {
    // Metadata
    source: PathBuf,
    metadata: Option<PluginMetadata>,
    progress: Progress,
    messages: VecDeque<String>,

    // Maintenance state
    task: Task<Result<()>>,
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

#[derive(Debug, Resource)]
pub struct SyncEngine {
    plugins: Vec<PluginHandle>,
    pool: MetadataPool,
}

impl SyncEngine {
    fn new(pool: MetadataPool) -> Self {
        Self {
            plugins: Vec::new(),
            pool,
        }
    }

    pub fn pool(&self) -> &MetadataPool {
        &self.pool
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

fn startup_sync_engine(
    metadata: Res<MetadataPoolResource>,
    env: Res<Environment>,
    mut commands: Commands,
) -> Result {
    let mut engine = SyncEngine::new(metadata.pool());

    let mut all_wasm = Vec::new();
    for item in fs::read_dir(env.global_plugin_dir())?.chain(fs::read_dir(env.local_plugin_dir())?)
    {
        let entry = item?;
        let is_wasm = entry
            .path()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with("wasm");
        if !is_wasm {
            continue;
        }
        all_wasm.push(entry.path());
    }

    for source in all_wasm.drain(..) {
        let state = UserData::new(PluginState::new(&env.cache_dir(), metadata.pool()));
        info!("Loading plugin: {}", source.display());
        let manifest = Manifest::new([Wasm::file(source.clone())]);
        let plugin = PluginBuilder::new(manifest)
            .with_wasi(true)
            .with_function("fetch_text", [PTR], [PTR], state.clone(), fetch_text)
            .build()?;
        let (tx_to_plugin, rx_from_runner) = channel::unbounded();
        let (tx_to_runner, rx_from_plugin) = channel::unbounded();
        let plugin_source = source.clone();
        let plugin_state = state.clone();
        let plugin_task = AsyncComputeTaskPool::get().spawn(async move {
            let rv = plugin_main(
                plugin,
                plugin_state.clone(),
                rx_from_runner,
                tx_to_runner.clone(),
            )
            .await;
            if let Err(e) = rv.as_ref() {
                let mut progress = ProgressSender::wrap(tx_to_runner);
                progress.message("Plugin shutting down");
                progress.message("Error: {e}");
                error!(
                    "Plugin {} shutting down with error: {}",
                    plugin_source.display(),
                    e
                );
            }
            rv
        });
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

fn maintain_plugins(mut engine: ResMut<SyncEngine>, app_exit: EventReader<AppExit>) -> Result {
    for handle in engine.plugins.iter_mut() {
        while let Ok(msg) = handle.rx_from_plugin.try_recv() {
            match msg {
                PluginResponse::PluginInfo(info) => {
                    handle.metadata = Some(info);
                }
                PluginResponse::Progress(progress) => {
                    handle.progress = progress;
                }
                PluginResponse::Message(message) => {
                    handle.messages.push_back(message);
                    while handle.messages.len() > PluginHandle::MAX_MESSAGES {
                        handle.messages.pop_front();
                    }
                }
            }
        }
    }

    // Proxy shutdown to our plugins and wait for them to terminate.
    if !app_exit.is_empty() {
        for handle in engine.plugins.drain(..) {
            handle.tx_to_plugin.send(PluginRequest::Shutdown)?;
            block_on(handle.task)?;
        }
    }

    Ok(())
}

// This is the entry point for the plugin loop. The plugin will run in its own thread, communicating
// with the main thread via the queues in the handle resource.
//
// Plugin Lifetime:
// * startup - return an information pack about the plugin and set it on the state for
//             display in the UX. This may contain required configuration fields to be
//             shown in the UX.
// * TODO: configure - receive configuration from the UX and store it in the plugin state.
// * Refresh* - query our plugin (to read from the gallery source) and write back the data
//              to the metadata db for display in the UX.
// * shutdown - return from the plugin thread so that we can cleanly shut down the pool and exit.
async fn plugin_main(
    mut plugin: ExtPlugin,
    state: UserData<PluginState>,
    rx_from_runner: channel::Receiver<PluginRequest>,
    tx_to_runner: channel::Sender<PluginResponse>,
) -> Result {
    let mut progress = ProgressSender::wrap(tx_to_runner.clone());

    progress.message("Getting plugin metadata");
    let metadata = plugin.call::<(), Json<PluginMetadata>>("startup", ())?.0;
    let plugin_id = state
        .get()?
        .lock()
        .unwrap()
        .pool
        .upsert_plugin(metadata.name())?;
    progress.message(format!(
        "Started plugin id:{plugin_id}, \"{}\"",
        metadata.name()
    ));
    tx_to_runner.send(PluginResponse::PluginInfo(metadata))?;

    while let Ok(msg) = rx_from_runner.recv() {
        match msg {
            PluginRequest::Shutdown => {
                progress.message("Shutting down plugin: {plugin_id}");
                break;
            }
            PluginRequest::RefreshTags => {
                let tags = plugin.call::<(), Json<Vec<String>>>("list_tags", ())?;
                progress.message(format!("Discovered {} tags", tags.0.len()));

                // Note: don't hold the lock while doing the long running db thing.
                let pool = state.get()?.lock().unwrap().pool.clone();
                pool.upsert_tags(plugin_id, &tags.0, &mut progress)?;
            }
        }
    }
    Ok(())
}

host_fn!(fetch_text(state: PluginState; url: &str) -> String {
    // Check our cache first
    let key = Sha256::digest(url.as_bytes());
    let keypath = PluginState::cache_dir(&state).join(format!("{key:x}"));
    if let Ok(data) = fs::read_to_string(&keypath) {
        // PluginState::message(&state, format!("Cache hit: {url}"));
        return Ok(data);
    }
    // PluginState::message(&state, format!("Fetching: {url}"));

    // Stream the response into our cache file
    let mut response= PluginState::agent(&state).get(url).call()?;
    let mut fp = fs::File::create(keypath.clone())?;
    io::copy(&mut response.body_mut().as_reader(), &mut fp)?;

    // Map the file and return a pointer to the contents
    Ok(fs::read_to_string(&keypath)?)
});
