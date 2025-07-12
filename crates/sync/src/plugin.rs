use crate::{
    model::MetadataPool,
    progress::ProgressSender,
    shared::{PluginMetadata, PluginRequest, PluginResponse},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use crossbeam::channel::{Receiver, Sender};
use extism::{
    Manifest, PTR, Plugin as ExtPlugin, PluginBuilder, UserData, Wasm, convert::Json, host_fn,
};
use io_tee::TeeWriter;
use progress_streams::{ProgressReader, ProgressWriter};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use ureq::Agent;

pub(crate) fn load_plugin(
    source: &Path,
    cache_dir: &Path,
    pool: MetadataPool,
    rx_from_runner: Receiver<PluginRequest>,
    tx_to_runner: Sender<PluginResponse>,
) -> Result<Task<()>> {
    info!("Loading plugin: {}", source.display());
    let progress = ProgressSender::wrap(tx_to_runner.clone());
    let state = UserData::new(PluginState::new(cache_dir, pool, progress.clone()));
    let manifest = Manifest::new([Wasm::file(source)]);
    let plugin = PluginBuilder::new(manifest)
        .with_wasi(true)
        .with_function("fetch_text", [PTR], [PTR], state.clone(), fetch_text)
        .build()?;

    let plugin_source = source.to_owned();
    let plugin_task = AsyncComputeTaskPool::get().spawn(async move {
        let rv = plugin_main(plugin, state.clone(), rx_from_runner, progress).await;
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
    });
    Ok(plugin_task)
}

#[derive(Debug)]
struct PluginState {
    cache_dir: PathBuf,
    pool: MetadataPool,
    agent: Agent,
    progress: ProgressSender,
}

impl PluginState {
    fn new(cache_dir: &Path, pool: MetadataPool, progress: ProgressSender) -> Self {
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
            progress,
        }
    }
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
    rx_from_runner: Receiver<PluginRequest>,
    mut progress: ProgressSender,
) -> Result {
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
    progress.send(PluginResponse::PluginInfo(metadata))?;

    while let Ok(msg) = rx_from_runner.recv() {
        match msg {
            PluginRequest::Shutdown => {
                progress.message("Shutting down plugin: {plugin_id}");
                break;
            }
            PluginRequest::RefreshTags => {
                // Progress will get sent for the download or file read.
                let mut tags = plugin.call::<(), Json<Vec<String>>>("list_tags", ())?.0;

                // FIXME: test with a large data set
                for i in 0..1_000_000 {
                    tags.push(format!("tag{i}"));
                }

                // Progress will get sent a second time for writing to the DB.
                state
                    .get()?
                    .lock()
                    .unwrap()
                    .pool
                    .upsert_tags(plugin_id, &tags, &mut progress)?;
            }
        }
    }
    Ok(())
}

host_fn!(fetch_text(state: PluginState; url: &str) -> String {
    // Note: it is fine to hold our plugin lock across long-running tasks;
    //       there is no conflict on this lock, by design.
    let state_ref = state.get().unwrap();
    let mut guard = state_ref.lock().unwrap();
    guard.progress.set_spinner();

    // Check our cache first
    let key = Sha256::digest(url.as_bytes());
    let keypath = guard.cache_dir.join(format!("{key:x}"));
    if let Ok(cache_fp) = fs::File::open(&keypath) {
        guard.progress.message(format!("Cache hit: {url}"));
        let cache_len = cache_fp.metadata()?.len() as usize;
        let mut cache_reader = ProgressReader::new(cache_fp, |progress: usize| {
            guard.progress.set_percent(progress, cache_len);
        });
        let mut buffer = Vec::new();
        io::copy(&mut cache_reader, &mut buffer).expect("Failed to read cached url");
        let out = String::from_utf8_lossy(&buffer).to_string();
        guard.progress.clear();
        return Ok(out);
    }

    // Stream the response simultaneously to the cache file and to a string for use by the plugin.
    guard.progress.message(format!("Downloading: {url}"));
    let mut response= guard.agent.get(url).call()?;
    let content_len = response.body().content_length();
    if let Some(content_len) = content_len {
        guard.progress.message(format!("Reading {content_len} bytes"));
    }
    let mut fp = fs::File::create(keypath.clone())?;
    let mut buffer = Vec::new();
    let mut tee = TeeWriter::new(&mut fp, &mut buffer);
    let mut writer = ProgressWriter::new(&mut tee, |progress: usize| {
        if let Some(content_len) = content_len {
            guard.progress.set_percent(progress, content_len as usize);
        }
    });
    io::copy(&mut response.body_mut().as_reader(), &mut writer)?;

    // Map the file and return a pointer to the contents
    let out = String::from_utf8_lossy(&buffer).to_string();
    guard.progress.clear();
    Ok(out)
});
