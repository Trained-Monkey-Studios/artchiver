use crate::{Environment, model::connect_or_create_db};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::FutureExt},
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
    MaintainPlugins,
}

pub struct SyncPlugin;
impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SyncEngine::default())
            .add_systems(Startup, (connect_or_create_db, startup_sync_engine))
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
    agent: Agent,
    messages: VecDeque<String>,
}

impl PluginState {
    const MAX_MESSAGES: usize = 20;

    fn new(cache_dir: &Path) -> Self {
        const VERSION: &'static str = env!("CARGO_PKG_VERSION");
        Self {
            cache_dir: cache_dir.to_owned(),
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
            messages: VecDeque::new(),
        }
    }

    fn cache_dir(state: &UserData<PluginState>) -> PathBuf {
        state.get().unwrap().lock().unwrap().cache_dir.clone()
    }

    fn agent(state: &UserData<PluginState>) -> Agent {
        state.get().unwrap().lock().unwrap().agent.clone()
    }

    fn message(state: &UserData<PluginState>, message: impl AsRef<str>) {
        let size = state.get().unwrap().lock().unwrap().messages.len();
        if size > Self::MAX_MESSAGES {
            let popcnt = size - Self::MAX_MESSAGES;
            for _ in 0..popcnt {
                state.get().unwrap().lock().unwrap().messages.pop_front();
            }
        }
        state
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .messages
            .push_back(message.as_ref().to_owned());
    }
}

#[derive(Clone, Debug)]
enum PluginRequest {
    Shutdown,
    ListTags,
}

#[derive(Clone, Debug)]
enum PluginResponse {
    PluginInfo(PluginMetadata),
    FoundTags(Vec<String>),
}

#[derive(Debug)]
pub struct PluginHandle {
    // Metadata
    source: PathBuf,
    metadata: Option<PluginMetadata>,
    state: UserData<PluginState>,

    // Maintenance state
    task: Task<Result<()>>,
    tx_to_plugin: channel::Sender<PluginRequest>,
    rx_from_plugin: channel::Receiver<PluginResponse>,
}

impl PluginHandle {
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

    pub fn messages(&self) -> Result<Vec<String>> {
        Ok(self
            .state
            .get()?
            .lock()
            .unwrap()
            .messages
            .iter()
            .cloned()
            .collect())
    }
}

#[derive(Debug, Default, Resource)]
pub struct SyncEngine {
    plugins: Vec<PluginHandle>,
}

impl SyncEngine {
    pub fn plugins(&self) -> impl Iterator<Item = &PluginHandle> {
        self.plugins.iter()
    }

    pub fn refresh_tags(&self) -> Result {
        for plugin in self.plugins.iter() {
            plugin.tx_to_plugin.send(PluginRequest::ListTags)?;
        }
        Ok(())
    }
}

fn startup_sync_engine(mut engine: ResMut<SyncEngine>, env: Res<Environment>) -> Result {
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
        info!("Loading plugin: {}", source.display());
        let state = UserData::new(PluginState::new(&env.cache_dir()));
        let manifest = Manifest::new([Wasm::file(source.clone())]);
        let plugin = PluginBuilder::new(manifest)
            .with_wasi(true)
            .with_function("fetch_text", [PTR], [PTR], state.clone(), fetch_text)
            .build()?;
        let (tx_to_plugin, rx_from_runner) = channel::unbounded();
        let (tx_to_runner, rx_from_plugin) = channel::unbounded();
        let plugin_task = AsyncComputeTaskPool::get()
            .spawn(async move { plugin_main(plugin, rx_from_runner, tx_to_runner).await });
        engine.plugins.push(PluginHandle {
            source,
            metadata: None,
            state,
            task: plugin_task,
            tx_to_plugin,
            rx_from_plugin,
        });
    }
    Ok(())
}

fn maintain_plugins(mut engine: ResMut<SyncEngine>, app_exit: EventReader<AppExit>) -> Result {
    for handle in engine.plugins.iter_mut() {
        while let Ok(msg) = handle.rx_from_plugin.try_recv() {
            match msg {
                PluginResponse::PluginInfo(info) => {
                    handle.metadata = Some(info);
                }
                PluginResponse::FoundTags(tags) => {
                    println!("TAGS: {tags:#?}");
                }
            }
        }
    }

    // Proxy shutdown to our plugins and wait for them to terminate.
    if !app_exit.is_empty() {
        for mut handle in engine.plugins.drain(..) {
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
// * startup - return the information pack
async fn plugin_main(
    mut plugin: ExtPlugin,
    rx_from_runner: channel::Receiver<PluginRequest>,
    tx_to_runner: channel::Sender<PluginResponse>,
) -> Result {
    let res = plugin.call::<(), Json<PluginMetadata>>("startup", ())?;
    tx_to_runner.send(PluginResponse::PluginInfo(res.0))?;

    while let Ok(msg) = rx_from_runner.recv() {
        match msg {
            PluginRequest::Shutdown => {
                break;
            }
            PluginRequest::ListTags => {
                let tags = plugin.call::<(), Json<Vec<String>>>("list_tags", ())?;
                tx_to_runner.send(PluginResponse::FoundTags(tags.0))?;
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
        PluginState::message(&state, format!("Cache hit: {url}"));
        return Ok(data);
    }
    PluginState::message(&state, format!("Fetching: {url}"));

    // Stream the response into our cache file
    let mut response= PluginState::agent(&state).get(url).call()?;
    let mut fp = fs::File::create(keypath.clone())?;
    io::copy(&mut response.body_mut().as_reader(), &mut fp)?;

    // Map the file and return a pointer to the contents
    Ok(fs::read_to_string(&keypath)?)
});
