use crate::sync::db::models::plugin::PluginId;
use crate::{
    shared::{
        environment::Environment,
        plugin::{PluginCancellation, PluginRequest},
        progress::{HostUpdateSender, LogSender, ProgressSender, UpdateSource},
        throttle::CallingThrottle,
        update::DataUpdate,
    },
    sync::{
        db::{sync::DbSyncHandle, writer::DbWriteHandle},
        plugin::download::download_works,
    },
};
use anyhow::Result;
use artchiver_sdk::{PluginMetadata, Request, Tag, TextFetchError, TextResponse, Work};
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
    db_sync: DbSyncHandle,
    db_write: DbWriteHandle,
    rx_from_runner: Receiver<PluginRequest>,
    tx_to_runner: Sender<DataUpdate>,
) -> Result<(JoinHandle<()>, PluginCancellation)> {
    info!("Loading plugin: {}", source.display());
    let state = UserData::new(PluginState::new(env, db_sync, db_write, tx_to_runner));
    let cancellation = state.get()?.lock().expect("poison").cancellation.clone();
    // Note: on configuration; we support moving the plugin file around, so we need to key on the
    //       name rather than the source path. As such, we have to wait until the plugin returns
    //       its metadata to us. At which point we look up the config and rebuild the plugin.
    let plugin = make_plugin(source, vec![], &state)?;

    let plugin_source = source.to_owned();
    let plugin_task = spawn(move || {
        let rv = plugin_main(&plugin_source, plugin, &state, &rx_from_runner);
        if let Err(e) = rv.as_ref() {
            let state_ref = state.get().expect("state dropped");
            let mut state = state_ref.lock().expect("poison");
            state.log.error("Plugin shutting down with error");
            state.log.error(format!("Error: {e}"));
        }
    });
    Ok((plugin_task, cancellation))
}

#[derive(Debug)]
pub struct PluginState {
    // Environment
    cache_dir: PathBuf,
    data_dir: PathBuf,
    tmp_dir: PathBuf,
    progress: ProgressSender,
    log: LogSender,
    host: HostUpdateSender,
    cancellation: PluginCancellation,

    // Database
    db_sync: DbSyncHandle,
    db_write: DbWriteHandle,

    // Web
    agent: Agent,
    throttle: CallingThrottle,
}

fn make_agent() -> Agent {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    Agent::new_with_config(
        Agent::config_builder()
            .no_delay(true)
            .user_agent(format!("Artchiver/{VERSION}"))
            .redirect_auth_headers(RedirectAuthHeaders::SameHost)
            .max_response_header_size(256 * 1024)
            .timeout_global(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .build(),
    )
}

impl PluginState {
    fn new(
        env: &Environment,
        db_sync: DbSyncHandle,
        db_write: DbWriteHandle,
        tx_to_runner: Sender<DataUpdate>,
    ) -> Self {
        Self {
            cache_dir: env.cache_dir().clone(),
            data_dir: env.data_dir().clone(),
            tmp_dir: env.tmp_dir().clone(),
            progress: ProgressSender::wrap(UpdateSource::Unknown, tx_to_runner.clone()),
            log: LogSender::wrap(UpdateSource::Unknown, tx_to_runner.clone()),
            host: HostUpdateSender::wrap(UpdateSource::Unknown, tx_to_runner),
            cancellation: PluginCancellation::default(),
            db_sync,
            db_write,
            agent: make_agent(),
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
//             shown in the UX. The plugin may be restarted between the call to startup
//             and other calls, so don't save pointers.
// * Refresh* - query our plugin (to read from the gallery source) and write back the data
//              to the metadata db for display in the UX.
// * shutdown - return from the plugin thread so that we can cleanly shut down and exit.
fn plugin_main(
    plugin_source: &Path,
    mut plugin: ExtPlugin,
    state: &UserData<PluginState>,
    rx_from_runner: &Receiver<PluginRequest>,
) -> Result<()> {
    let mut metadata = plugin.call::<(), Json<PluginMetadata>>("startup", ())?.0;
    let db_plugin = {
        let state_ref = state.get()?;
        let mut state = state_ref.lock().expect("poison");
        let db_plugin = state.db_sync.sync_upsert_plugin(metadata.name())?;
        state.throttle = CallingThrottle::new(metadata.rate_limit(), metadata.rate_window());
        state.progress = ProgressSender::wrap(
            UpdateSource::Plugin(db_plugin.id()),
            state.progress.channel(),
        );
        state.log = LogSender::wrap(UpdateSource::Plugin(db_plugin.id()), state.log.channel());
        state.host =
            HostUpdateSender::wrap(UpdateSource::Plugin(db_plugin.id()), state.host.channel());
        for (k, v) in db_plugin.configs() {
            metadata.set_config_value(k, v);
        }
        db_plugin
    };
    let mut progress = state.get()?.lock().expect("poison").progress.clone();
    let mut log = state.get()?.lock().expect("poison").log.clone();
    let mut host = state.get()?.lock().expect("poison").host.clone();

    // Note: restart plugin with configuration in place this time
    plugin = make_plugin(plugin_source, metadata.configurations().to_owned(), state)?;
    log.info(format!(
        "Started plugin id:{}, \"{}\"",
        db_plugin.id(),
        metadata.name()
    ));
    host.plugin_loaded(plugin_source, &db_plugin, &metadata)?;

    'outer: while let Ok(msg) = rx_from_runner.recv() {
        let rv = match msg {
            PluginRequest::Shutdown => {
                log.info(format!("Shutting down plugin: {}", db_plugin.id()));
                break 'outer;
            }
            PluginRequest::ApplyConfiguration { config } => {
                state
                    .get()?
                    .lock()
                    .expect("poison")
                    .db_sync
                    .sync_save_configurations(db_plugin.id(), &config)?;
                // reload the plugin with configuration applied
                plugin = make_plugin(plugin_source, config, state)?;
                Ok(())
            }
            PluginRequest::RefreshTags => {
                refresh_tags(db_plugin.id(), &mut plugin, state, &mut log)
            }
            PluginRequest::RefreshWorksForTag { tag } => {
                refresh_works_for_tag(db_plugin.id(), &tag, &mut plugin, state)
            }
        };
        if let Err(e) = rv {
            log.error(format!("Error handling plugin message: {e}"));
            // Note: reset the agent if we fail, to hopefully break any bad connections.
            state.get()?.lock().expect("poison").agent = make_agent();
        }
        // Note: we want to fail and crash out of the plugin if nobody is listening.
        host.note_completed_task()?;
        // Note: always reset the cancellation on task complete. If we missed hitting
        //       a trigger, it no longer matters once we get to this point.
        state.get()?.lock().expect("poison").cancellation.reset();
        // And ditto for our progress situation, particularly if we were canceled.
        progress.clear();
    }
    Ok(())
}

fn refresh_tags(
    plugin_id: PluginId,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
    log: &mut LogSender,
) -> Result<()> {
    // Progress will get sent for the download or file read.
    log.trace(format!("Calling plugin ({plugin_id}) -> list_tags"));
    let tags = plugin.call::<(), Json<Vec<Tag>>>("list_tags", ())?.0;

    // Progress will get sent a second time for writing to the DB.
    let state_ref = state.get()?;
    let state = state_ref.lock().expect("poison");
    state.db_write.upsert_tags(plugin_id, tags)?;

    Ok(())
}

fn refresh_works_for_tag(
    plugin_id: PluginId,
    tag: &str,
    plugin: &mut ExtPlugin,
    state: &UserData<PluginState>,
) -> Result<()> {
    let (data_dir, tmp_dir, db, agent, mut progress, mut log, throttle, cancellation) = {
        let state_ref = state.get()?;
        let state = state_ref.lock().expect("poison");
        (
            state.data_dir.clone(),
            state.tmp_dir.clone(),
            state.db_write.clone(),
            state.agent.clone(),
            state.progress.clone(),
            state.log.clone(),
            state.throttle.clone(),
            state.cancellation.clone(),
        )
    };

    // Ask the plugin to figure out what works we have for this tag.
    progress.set_spinner();
    log.trace(format!("Calling plugin->list_works_for_tag(\"{tag}\")"));
    let works = plugin
        .call::<String, Json<Vec<Work>>>("list_works_for_tag", tag.to_owned())?
        .0;

    // Save the works we found.
    log.trace(format!("Saving {} works to Database async", works.len()));
    db.upsert_works(plugin_id, tag, works.clone())?;

    // Fetch all images
    // Note: we don't need to wait for the upsert to happen before we start downloading, since
    //       all we need in the urls and those are in the Work. As we download files, the messages
    //       to update the local paths will just queue up behind the upsert.
    download_works(
        works,
        &db,
        (&agent, &throttle),
        (&data_dir, &tmp_dir),
        (&mut progress, &mut log, &cancellation),
    )?;
    log.info(format!("Finished download tag {tag}..."));

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
    state.get()?.lock().expect("poison").log.log_message(level, msg);
    Ok(())
});

fn fetch_text_inner(state: &mut PluginState, request: &Request) -> TextResponse {
    let url = request.to_url();

    // Check our cache first
    let key = Sha256::digest(&url);
    let key_path = state.cache_dir.join(format!("{key:x}"));
    if let Ok(mut cache_fp) = fs::File::open(&key_path) {
        // state.log.trace(format!("cached: fetch_text({url})"));
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
    state.log.trace(format!("fetch_text({url})"));
    let tmp_path = make_temp_path(&state.tmp_dir);
    let buffer = {
        let mut req = state.agent.get(&url);
        for (key, value) in request.headers() {
            req = req.header(key, value);
        }
        let mut response = match req.call() {
            Ok(response) => response,
            Err(e) => {
                state.log.error(format!("Request failed for {url}: {e}"));
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

pub fn make_temp_path(tmp_dir: &Path) -> PathBuf {
    let tmp_name: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect();
    tmp_dir.join(tmp_name)
}
