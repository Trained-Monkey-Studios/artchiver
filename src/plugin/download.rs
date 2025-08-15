use crate::{
    db::writer::DbWriteHandle,
    plugin::client::make_temp_path,
    shared::{
        plugin::PluginCancellation,
        progress::{LogSender, ProgressSender},
        throttle::CallingThrottle,
    },
};
use anyhow::{Context as _, Result};
use artchiver_sdk::Work;
use rayon::ThreadPool;
use sha2::{Digest as _, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use ureq::Agent;

pub fn download_works(
    mut works: Vec<Work>,
    db: &DbWriteHandle,
    pool: &ThreadPool,
    (agent, throttle): (&Agent, &CallingThrottle),
    (data_dir, tmp_dir): (&Path, &Path),
    (progress, log, cancellation): (&mut ProgressSender, &mut LogSender, &PluginCancellation),
) -> Result<()> {
    log.info(format!("Downloading {} works to disk...", works.len()));
    let works_len = works.len();

    // rayon::scope_fifo(|s| {
    pool.scope_fifo(|s| {
        for (i, work) in works.drain(..).enumerate() {
            let mut progress = progress.clone();
            let mut log = log.clone();
            s.spawn_fifo(move |_| {
                progress.set_percent(i, works_len);
                if let Err(e) = ensure_work_data_is_cached(
                    &work,
                    db,
                    (agent, throttle),
                    (data_dir, tmp_dir),
                    (&mut log.clone(), cancellation),
                ) {
                    // Note: ignore download failures and let the user re-try, if needed.
                    log.error(format!(
                        "Error downloading work {}: {e}\n{}",
                        work.name(),
                        e.backtrace()
                    ));
                }

                // FIXME: we need to send this from the db side so that we (1) only show things
                //        that are actually saved permanently and (2) so that we have the WorkId
                // TODO: send back changes as they occur
                // host.finished_download_for(id)
            });
        }
    });
    Ok(())
}

// Returns the absolute path for I/O and the relative path in the data directory for metadata.
pub fn get_data_path_for_url(data_dir: &Path, url: &str) -> Result<(PathBuf, String)> {
    let ext = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .rsplit('.')
        .next()
        .unwrap_or_default();
    let key = Sha256::digest(url.as_bytes());
    let key = format!("{key:x}");
    let level1 = &key[0..2];
    let level2 = &key[2..4];
    let file_base = &key[4..];
    let relative = format!("{level1}/{level2}/{file_base}.{ext}");
    let dir_path = data_dir.join(level1).join(level2);
    fs::create_dir_all(&dir_path).context("failed to create data path dirs")?;
    Ok((data_dir.join(&relative), relative))
}

fn ensure_work_data_is_cached(
    work: &Work,
    db: &DbWriteHandle,
    (agent, throttle): (&Agent, &CallingThrottle),
    (data_dir, tmp_dir): (&Path, &Path),
    (log, cancellation): (&mut LogSender, &PluginCancellation),
) -> Result<()> {
    let preview_path = ensure_data_url(
        work.preview_url(),
        data_dir,
        tmp_dir,
        agent,
        log,
        throttle,
        cancellation,
    )?;

    let screen_path = ensure_data_url(
        work.screen_url(),
        data_dir,
        tmp_dir,
        agent,
        log,
        throttle,
        cancellation,
    )?;

    // FIXME: figure out how to download an iiif tiled image.
    let archive_path = None;
    // let archive_path = if let Some(archive_url) = work.archive_url() {
    //     Some(ensure_data_url(
    //         archive_url,
    //         data_dir,
    //         tmp_dir,
    //         agent,
    //         log,
    //         throttle,
    //         cancellation,
    //     )?)
    // } else {
    //     None
    // };

    db.set_work_download_paths(work.screen_url(), preview_path, screen_path, archive_path)?;
    Ok(())
}

// Reads the data to disk and returns the data-dir-relative path for storage.
fn ensure_data_url(
    url: &str,
    data_dir: &Path,
    tmp_dir: &Path,
    agent: &Agent,
    log: &mut LogSender,
    throttle: &CallingThrottle,
    cancellation: &PluginCancellation,
) -> Result<String> {
    let (abs_path, rel_path) = get_data_path_for_url(data_dir, url)?;
    if abs_path.exists() {
        // log.trace(format!("cached: ensure_data_url({url})"));
        return Ok(rel_path);
    }

    // Note: check throttle before opening files, etc, but after we might bail for caching.
    throttle.throttle(cancellation)?;

    let tmp_path = make_temp_path(tmp_dir);
    {
        // Note: in a block to Drop, to close the file before renaming it, just for sanity.
        let tmp_fp = fs::File::create(&tmp_path).context("failed to create temporary file")?;
        log.trace(format!("ensure_data_url({url})"));
        let mut resp = agent.get(url).call()?;
        io::copy(
            &mut resp.body_mut().as_reader(),
            &mut io::BufWriter::new(tmp_fp),
        )
        .context("failed to download file")?;
    }
    fs::rename(&tmp_path, &abs_path)
        .with_context(|| format!("failed to rename temporary file {tmp_path:?} -> {abs_path:?}"))?;
    Ok(rel_path)
}
