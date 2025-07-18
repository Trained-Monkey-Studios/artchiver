use crate::{
    shared::{
        environment::Environment,
        plugin::{PluginCancellation, PluginRequest, PluginResponse},
        progress::ProgressSender,
        throttle::CallingThrottle,
    },
    sync::db::{model::MetadataPool, tag::upsert_tags},
};
use anyhow::Result;
use artchiver_sdk::{PluginMetadata, Request, TagInfo, TextFetchError, TextResponse, Work};
use crossbeam::channel::{Receiver, Sender};
use extism::{
    Manifest, PTR, Plugin as ExtPlugin, PluginBuilder, UserData, Wasm, convert::Json, host_fn,
};
use io_tee::TeeWriter;
use log::info;
use rand::{Rng as _, distr::Alphanumeric};
use sha2::{Digest as _, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread::{JoinHandle, spawn},
    time::Duration,
};
use ureq::{Agent, config::RedirectAuthHeaders};

fn make_plugin(
    source: &Path,
    config: Vec<(String, String)>,
    state: &UserData<PluginState>,
) -> Result<ExtPlugin> {
    let manifest = Manifest::new([Wasm::file(source)]).with_config(config.into_iter());
    let plugin = PluginBuilder::new(manifest)
        .with_wasi(true)
        .with_function("progress_spinner", [], [], state.clone(), progress_spinner)
        .with_function(
            "progress_percent",
            [PTR, PTR],
            [],
            state.clone(),
            progress_percent,
        )
        .with_function("progress_clear", [], [], state.clone(), progress_clear)
        .with_function("log_message", [PTR, PTR], [], state.clone(), log_message)
        .with_function("fetch_text", [PTR], [PTR], state.clone(), fetch_text)
        .build()?;
    Ok(plugin)
}

pub(crate) fn create_plugin_task(
    source: &Path,
    env: &Environment,
    pool: MetadataPool,
    rx_from_runner: Receiver<PluginRequest>,
    tx_to_runner: Sender<PluginResponse>,
) -> Result<(JoinHandle<()>, PluginCancellation)> {
    info!("Loading plugin: {}", source.display());
    let progress = ProgressSender::wrap(tx_to_runner.clone());
    let state = UserData::new(PluginState::new(env, pool, progress.clone()));
    let cancellation = state.get()?.lock().expect("poison").cancellation.clone();
    // Note: on configuration; we support moving the plugin file around, so we need to key on the
    //       name rather than the source path. As such, we have to wait until the plugin returns
    //       its metadata to us. At which point we look up the config and rebuild the plugin.
    let plugin = make_plugin(source, vec![], &state)?;

    let plugin_source = source.to_owned();
    let plugin_task = spawn(move || {
        let rv = plugin_main(&plugin_source, plugin, &state, &rx_from_runner, progress);
        if let Err(e) = rv.as_ref() {
            let mut progress = ProgressSender::wrap(tx_to_runner);
            progress.error("Plugin shutting down");
            progress.error(format!("Error: {e}"));
        }
    });
    Ok((plugin_task, cancellation))
}

#[derive(Debug)]
struct PluginState {
    // Environment
    cache_dir: PathBuf,
    data_dir: PathBuf,
    tmp_dir: PathBuf,
    progress: ProgressSender,
    cancellation: PluginCancellation,

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
            cache_dir: env.cache_dir().clone(),
            data_dir: env.data_dir().clone(),
            tmp_dir: env.tmp_dir().clone(),
            progress,
            cancellation: PluginCancellation::default(),
            pool,
            agent: Agent::new_with_config(
                Agent::config_builder()
                    .no_delay(true)
                    .user_agent(format!("Artchiver/{VERSION}"))
                    .redirect_auth_headers(RedirectAuthHeaders::SameHost)
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
fn plugin_main(
    plugin_source: &Path,
    mut plugin: ExtPlugin,
    state: &UserData<PluginState>,
    rx_from_runner: &Receiver<PluginRequest>,
    mut progress: ProgressSender,
) -> Result<()> {
    progress.info(format!("Starting plugin {}", plugin_source.display()));
    let mut metadata = plugin.call::<(), Json<PluginMetadata>>("startup", ())?.0;
    let plugin_id = {
        let state_ref = state.get()?;
        let mut state = state_ref.lock().expect("poison");
        state.throttle = CallingThrottle::new(metadata.rate_limit(), metadata.rate_window());
        let plugin_id = state.pool.upsert_plugin(metadata.name())?;
        let configs = state.pool.load_configurations(plugin_id)?;
        for (k, v) in &configs {
            metadata.set_config_value(k, v);
        }
        plugin_id
    };
    // Note: restart plugin with configuration in place this time
    plugin = make_plugin(plugin_source, metadata.configurations().to_owned(), state)?;
    progress.info(format!(
        "Started plugin id:{plugin_id}, \"{}\"",
        metadata.name()
    ));
    progress.send(PluginResponse::PluginInfo(metadata))?;

    'outer: while let Ok(msg) = rx_from_runner.recv() {
        let rv = match msg {
            PluginRequest::Shutdown => {
                progress.info(format!("Shutting down plugin: {plugin_id}"));
                break 'outer;
            }
            PluginRequest::ApplyConfiguration { config } => {
                state
                    .get()?
                    .lock()
                    .expect("poison")
                    .pool
                    .save_configurations(plugin_id, &config)?;
                // reload the plugin with configuration applied
                plugin = make_plugin(plugin_source, config, state)?;
                Ok(())
            }
            PluginRequest::RefreshTags => {
                refresh_tags(plugin_id, &mut plugin, state, &mut progress)
            }
            PluginRequest::RefreshWorksForTag { tag } => {
                refresh_works_for_tag(&tag, &mut plugin, state, &mut progress)
            }
        };
        if let Err(e) = rv {
            progress.error(format!("Error handling plugin message: {e}"));
        }
        // Note: we want to fail and crash out of the plugin if nobody is listening.
        progress.note_completed_task()?;
        // Note: always reset the cancellation on task complete. If we missed hitting
        //       a trigger, it no longer matters once we get to this point.
        state.get()?.lock().expect("poison").cancellation.reset();
        // And ditto for our progress situation, particularly if we were canceled.
        progress.clear();
    }
    Ok(())
}

fn refresh_tags(
    plugin_id: i64,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    progress: &mut ProgressSender,
) -> Result<()> {
    // Progress will get sent for the download or file read.
    progress.trace("Calling plugin->list_tags");
    let tags = plugin.call::<(), Json<Vec<TagInfo>>>("list_tags", ())?.0;

    // Progress will get sent a second time for writing to the DB.
    let state_ref = state.get()?;
    let state = state_ref.lock().expect("poison");
    upsert_tags(&state.pool.get()?, plugin_id, &tags, progress)?;

    Ok(())
}

fn refresh_works_for_tag(
    tag: &str,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    progress: &mut ProgressSender,
) -> Result<()> {
    // Ask the plugin to figure out what works we have for this tag.
    progress.set_spinner();
    progress.trace(format!("Calling plugin->list_tags_for_work(\"{tag}\")"));
    let works = plugin
        .call::<String, Json<Vec<Work>>>("list_works_for_tag", tag.to_owned())?
        .0;

    // Save the works we found.
    {
        let state_ref = state.get()?;
        let mut state = state_ref.lock().expect("poison");
        state.pool.upsert_works(&works, progress)?;
    }

    // Fetch preview images eagerly.
    progress.info(format!("Downloading {} works to disk...", works.len()));
    let works_len = works.len();
    let mut works = works;
    rayon::scope(|s| {
        for (i, work) in works.drain(..).enumerate() {
            let mut progress = progress.clone();
            s.spawn(move |_| {
                progress.set_percent(i, works_len);
                if let Err(e) = ensure_work_data_is_cached(state, &work) {
                    // Note: ignore download failures and let the user re-try, if needed.
                    progress.error(format!("Error downloading work {}: {e}", work.name()));
                }
            });
        }
    });
    progress.clear();
    Ok(())
}

host_fn!(progress_spinner(state: PluginState;) {
    state.get()?.lock().expect("poison").progress.set_spinner();
    Ok(())
});

host_fn!(progress_percent(state: PluginState; current: i32, total: i32) {
    state.get()?.lock().expect("poison").progress.set_percent(current.try_into()?, total.try_into()?);
    Ok(())
});

host_fn!(progress_clear(state: PluginState;) {
    state.get()?.lock().expect("poison").progress.clear();
    Ok(())
});

host_fn!(log_message(state: PluginState; level: u32, msg: String) {
    state.get()?.lock().expect("poison").progress.log_message(level, msg);
    Ok(())
});

fn fetch_text_inner(state: &mut PluginState, request: &Request) -> TextResponse {
    let url = request.to_url();

    // Check our cache first
    let key = Sha256::digest(&url);
    let key_path = state.cache_dir.join(format!("{key:x}"));
    if let Ok(mut cache_fp) = fs::File::open(&key_path) {
        state.progress.trace(format!("cached: fetch_text({url})"));
        let mut buffer = Vec::new();
        io::copy(&mut cache_fp, &mut buffer)?;
        let out = String::from_utf8_lossy(&buffer).to_string();
        return Ok(out);
    }

    // Stream the response simultaneously to the cache file and to a string for use by the plugin.
    state
        .throttle
        .throttle(&state.cancellation)
        .map_err(|_e| TextFetchError::Cancellation)?;
    state.progress.trace(format!("fetch_text({url})"));
    let tmp_path = make_temp_path(&state.tmp_dir);
    let buffer = {
        let mut req = state.agent.get(&url);
        for (key, value) in request.headers() {
            req = req.header(key, value);
        }
        let mut response = match req.call() {
            Ok(response) => response,
            Err(e) => {
                state
                    .progress
                    .error(format!("Request failed for {url}: {e}"));
                return Err(e.into());
            }
        };
        let mut tmp_fp = fs::File::create(&tmp_path)?;
        let mut buffer = Vec::new();
        let mut tee = TeeWriter::new(&mut tmp_fp, &mut buffer);
        io::copy(&mut response.body_mut().as_reader(), &mut tee)?;
        String::from_utf8_lossy(&buffer).to_string()
    };
    fs::rename(&tmp_path, &key_path)?;
    Ok(buffer)
}

host_fn!(fetch_text(state: PluginState; req: Json<Request>) -> Json<TextResponse> {
    // Note: it is fine to hold our plugin lock across long-running tasks;
    //       there is no conflict on this lock, by design.
    Ok(Json(fetch_text_inner(&mut state.get()?.lock().expect("poison"), &req.0)))
});

fn ensure_work_data_is_cached(state: &UserData<PluginState>, work: &Work) -> Result<()> {
    ensure_data_url(state, work.preview_url())?;
    ensure_data_url(state, work.screen_url())?;
    if let Some(archive_url) = work.archive_url() {
        ensure_data_url(state, archive_url)?;
    }
    Ok(())
}

pub fn get_data_path_for_url(data_dir: &Path, url: &str) -> PathBuf {
    let ext = url.rsplit('.').next().unwrap_or_default();
    let key = Sha256::digest(url.as_bytes());
    data_dir.join(format!("{key:x}.{ext}"))
}

fn make_temp_path(tmp_dir: &Path) -> PathBuf {
    let tmp_name: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect();
    tmp_dir.join(tmp_name)
}

fn ensure_data_url(state: &UserData<PluginState>, url: &str) -> Result<()> {
    // Take our lock early to extract data and sub-Arc-mutexes for progress, throttle, etc.
    let (data_dir, tmp_dir, agent, mut progress, throttle, cancellation) = {
        let state_ref = state.get()?;
        let state = state_ref.lock().expect("poison");
        (
            state.data_dir.clone(),
            state.tmp_dir.clone(),
            state.agent.clone(),
            state.progress.clone(),
            state.throttle.clone(),
            state.cancellation.clone(),
        )
    };

    let data_path = get_data_path_for_url(&data_dir, url);
    if data_path.exists() {
        progress.trace(format!("cached: ensure_data_url({url})"));
        return Ok(());
    }

    // Note: check throttle before opening files, etc, but after we might bail for caching.
    throttle.throttle(&cancellation)?;

    let tmp_path = make_temp_path(&tmp_dir);
    {
        // Note: in a block to Drop, to close the file before renaming it, just for sanity.
        let tmp_fp = fs::File::create(&tmp_path)?;
        progress.trace(format!("ensure_data_url({url})"));
        let mut resp = agent.get(url).call()?;
        io::copy(
            &mut resp.body_mut().as_reader(),
            &mut io::BufWriter::new(tmp_fp),
        )?;
    }
    fs::rename(&tmp_path, &data_path)?;
    Ok(())
}
