use crate::throttle::CallingThrottle;
use crate::{
    environment::Environment,
    model::MetadataPool,
    progress::ProgressSender,
    shared::{PluginRequest, PluginResponse},
};
use artchiver_sdk::{HttpTextResult, PluginMetadata, Work};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use crossbeam::channel::{Receiver, Sender};
use extism::{
    Manifest, PTR, Plugin as ExtPlugin, PluginBuilder, UserData, ValType, Wasm, convert::Json,
    host_fn,
};
use io_tee::TeeWriter;
use progress_streams::{ProgressReader, ProgressWriter};
use rand::{Rng, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use ureq::Agent;

pub(crate) fn load_plugin(
    source: &Path,
    env: &Environment,
    pool: MetadataPool,
    rx_from_runner: Receiver<PluginRequest>,
    tx_to_runner: Sender<PluginResponse>,
) -> Result<Task<()>> {
    info!("Loading plugin: {}", source.display());
    let progress = ProgressSender::wrap(tx_to_runner.clone());
    let state = UserData::new(PluginState::new(env, pool, progress.clone()));
    let manifest = Manifest::new([Wasm::file(source)]);
    let plugin = PluginBuilder::new(manifest)
        .with_wasi(true)
        .with_function("progress_spinner", [], [], state.clone(), progress_spinner)
        .with_function(
            "progress_percent",
            [ValType::I32, ValType::I32],
            [],
            state.clone(),
            progress_percent,
        )
        .with_function("progress_clear", [], [], state.clone(), progress_clear)
        .with_function("fetch_text", [PTR], [PTR], state.clone(), fetch_text)
        .with_function(
            "fetch_large_text",
            [PTR],
            [PTR],
            state.clone(),
            fetch_large_text,
        )
        .build()?;

    let plugin_source = source.to_owned();
    let plugin_task = AsyncComputeTaskPool::get().spawn(async move {
        let rv = plugin_main(plugin, state.clone(), rx_from_runner, progress).await;
        if let Err(e) = rv.as_ref() {
            let mut progress = ProgressSender::wrap(tx_to_runner);
            progress.message("Plugin shutting down");
            progress.message(format!("Error: {e}"));
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
    // Environment
    cache_dir: PathBuf,
    data_dir: PathBuf,
    tmp_dir: PathBuf,
    progress: ProgressSender,

    // Database
    pool: MetadataPool,

    // Web
    agent: Agent,
    throttle: CallingThrottle,
}

impl PluginState {
    fn new(env: &Environment, pool: MetadataPool, progress: ProgressSender) -> Self {
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        Self {
            cache_dir: env.cache_dir().to_owned(),
            data_dir: env.data_dir().to_owned(),
            tmp_dir: env.tmp_dir().to_owned(),
            progress,
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
            throttle: CallingThrottle::default(),
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
    let plugin_id = {
        let state_ref = state.get()?;
        let mut state = state_ref.lock().unwrap();
        state.throttle = CallingThrottle::new(metadata.rate_limit(), Duration::from_secs(1));
        state.pool.upsert_plugin(metadata.name())?
    };
    progress.message(format!(
        "Started plugin id:{plugin_id}, \"{}\"",
        metadata.name()
    ));
    progress.send(PluginResponse::PluginInfo(metadata))?;

    while let Ok(msg) = rx_from_runner.recv() {
        if matches!(msg, PluginRequest::Shutdown) {
            progress.message("Shutting down plugin: {plugin_id}");
            break;
        }
        if let Err(e) = handle_plugin_message(msg, plugin_id, &mut plugin, &state, &mut progress) {
            progress.message(format!("Error handling plugin message: {e}"));
            error!("Error handling plugin message: {e}");
        }
    }
    Ok(())
}

fn handle_plugin_message(
    msg: PluginRequest,
    plugin_id: i64,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    progress: &mut ProgressSender,
) -> Result {
    trace!("Handling plugin message: {:?}", msg);
    match msg {
        PluginRequest::Shutdown => Ok(()),
        PluginRequest::RefreshTags => refresh_tags(plugin_id, plugin, state, progress),
        PluginRequest::RefreshWorksForTag { tag } => {
            refresh_works_for_tag(tag, plugin, state, progress)
        }
    }
}

fn refresh_tags(
    plugin_id: i64,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    progress: &mut ProgressSender,
) -> Result<()> {
    // Progress will get sent for the download or file read.
    progress.trace("Calling plugin->list_tags");
    let tags = plugin.call::<(), Json<Vec<String>>>("list_tags", ())?.0;

    // Progress will get sent a second time for writing to the DB.
    let state_ref = state.get()?;
    let mut state = state_ref.lock().unwrap();
    state.pool.upsert_tags(plugin_id, &tags, progress)?;

    Ok(())
}

fn refresh_works_for_tag(
    tag: String,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    progress: &mut ProgressSender,
) -> Result<()> {
    // Ask the plugin to figure out what works we have for this tag.
    progress.set_spinner();
    progress.trace(format!("Calling plugin->list_tags_for_work(\"{tag}\")"));
    let works = plugin
        .call::<String, Json<Vec<Work>>>("list_works_for_tag", tag.clone())?
        .0;

    // Save the works we found.
    let state_ref = state.get()?;
    let mut state = state_ref.lock().unwrap();
    state.pool.upsert_works(&tag, &works, progress)?;

    // Fetch preview images eagerly.
    progress.message(format!("Downloading {} works to disk...", works.len()));
    for (i, work) in works.iter().enumerate() {
        progress.set_percent(i, works.len());
        if let Err(e) = ensure_work_data_is_cached(&state, work, progress) {
            progress.message(format!("Error downloading work {}: {e}", work.name()));
            error!("Error downloading work {}: {e}", work.name());
        }
    }
    progress.clear();

    Ok(())
}

host_fn!(progress_spinner(state: PluginState;) {
    state.get()?.lock().unwrap().progress.set_spinner();
    Ok(())
});

host_fn!(progress_percent(state: PluginState; current: i32, total: i32) {
    state.get()?.lock().unwrap().progress.set_percent(current.try_into()?, total.try_into()?);
    Ok(())
});

host_fn!(progress_clear(state: PluginState;) {
    state.get()?.lock().unwrap().progress.clear();
    Ok(())
});

fn fetch_text_inner(state: &mut PluginState, url: &str, progress: bool) -> Result<String> {
    if progress {
        state.progress.set_spinner();
    }

    // Check our cache first
    let key = Sha256::digest(url.as_bytes());
    let key_path = state.cache_dir.join(format!("{key:x}"));
    if let Ok(mut cache_fp) = fs::File::open(&key_path) {
        state.progress.trace(format!("cached: fetch_text({url})"));
        if progress {
            state.progress.message(format!("Cache hit: {url}"));
            let cache_len = cache_fp.metadata()?.len() as usize;
            let mut cache_reader = ProgressReader::new(cache_fp, |progress: usize| {
                state.progress.set_percent(progress, cache_len);
            });
            let mut buffer = Vec::new();
            io::copy(&mut cache_reader, &mut buffer)?;
            let out = String::from_utf8_lossy(&buffer).to_string();
            state.progress.clear();
            return Ok(out);
        } else {
            let mut buffer = Vec::new();
            io::copy(&mut cache_fp, &mut buffer)?;
            let out = String::from_utf8_lossy(&buffer).to_string();
            return Ok(out);
        }
    }

    // Stream the response simultaneously to the cache file and to a string for use by the plugin.
    state.progress.trace(format!("fetch_text({url})"));
    let tmp_path = make_temp_path(&state.tmp_dir);
    state.throttle.throttle();
    let buffer = if progress {
        let mut response = state.agent.get(url).call()?;
        let content_len = response.body().content_length();
        if let Some(content_len) = content_len {
            state
                .progress
                .message(format!("Reading {content_len} bytes"));
        }
        let mut tmp_fp = fs::File::create(&tmp_path)?;
        let mut buffer = Vec::new();
        let mut tee = TeeWriter::new(&mut tmp_fp, &mut buffer);
        let mut writer = ProgressWriter::new(&mut tee, |progress: usize| {
            if let Some(content_len) = content_len {
                state.progress.set_percent(progress, content_len as usize);
            }
        });
        io::copy(&mut response.body_mut().as_reader(), &mut writer)?;
        let out = String::from_utf8_lossy(&buffer).to_string();
        state.progress.clear();
        out
    } else {
        let mut response = state.agent.get(url).call()?;
        let mut tmp_fp = fs::File::create(&tmp_path)?;
        let mut buffer = Vec::new();
        let mut tee = TeeWriter::new(&mut tmp_fp, &mut buffer);
        io::copy(&mut response.body_mut().as_reader(), &mut tee)?;
        String::from_utf8_lossy(&buffer).to_string()
    };
    fs::rename(&tmp_path, &key_path)?;
    Ok(buffer)
}

host_fn!(fetch_text(state: PluginState; url: &str) -> Json<HttpTextResult> {
    // Note: it is fine to hold our plugin lock across long-running tasks;
    //       there is no conflict on this lock, by design.
    Ok(Json(match fetch_text_inner(&mut state.get()?.lock().unwrap(), url, false) {
        Ok(s) => HttpTextResult::Ok(s),
        Err(e) => HttpTextResult::Err { status_code: 0, message: e.to_string() }
    }))
});

host_fn!(fetch_large_text(state: PluginState; url: &str) -> Json<HttpTextResult> {
    // Note: it is fine to hold our plugin lock across long-running tasks;
    //       there is no conflict on this lock, by design.
    Ok(Json(match fetch_text_inner(&mut state.get()?.lock().unwrap(), url, true) {
        Ok(s) => HttpTextResult::Ok(s),
        Err(e) => HttpTextResult::Err { status_code: 0, message: e.to_string() }
    }))
});

fn ensure_work_data_is_cached(
    state: &PluginState,
    work: &Work,
    progress: &mut ProgressSender,
) -> Result<()> {
    ensure_data_url(state, work.preview_url(), progress)?;
    ensure_data_url(state, work.screen_url(), progress)?;
    if let Some(archive_url) = work.archive_url() {
        ensure_data_url(state, archive_url, progress)?;
    }
    Ok(())
}

pub fn get_data_path_for_url(data_dir: &Path, url: &str) -> Result<PathBuf> {
    let ext = url.rsplit('.').next().unwrap_or_default();
    let key = Sha256::digest(url.as_bytes());
    let data_path = data_dir.join(format!("{key:x}.{ext}"));
    Ok(data_path)
}

fn make_temp_path(tmp_dir: &Path) -> PathBuf {
    let tmp_name: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    tmp_dir.join(tmp_name)
}

fn ensure_data_url(
    state: &PluginState,
    url: &str,
    progress: &mut ProgressSender,
) -> Result<PathBuf> {
    let data_path = get_data_path_for_url(&state.data_dir, url)?;
    if data_path.exists() {
        progress.trace(format!("cached: ensure_data_url({url})"));
        return Ok(data_path);
    }

    let tmp_path = make_temp_path(&state.tmp_dir);
    {
        let tmp_fp = fs::File::create(&tmp_path)?;
        state.throttle.throttle();
        progress.trace(format!("ensure_data_url({url})"));
        let mut response = state.agent.get(url).call()?;
        io::copy(
            &mut response.body_mut().as_reader(),
            &mut io::BufWriter::new(tmp_fp),
        )?;
    }
    fs::rename(&tmp_path, &data_path)?;
    Ok(data_path)
}
