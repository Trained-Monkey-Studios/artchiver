use crate::{
    db::writer::DbWriteHandle,
    plugin::{client::make_temp_path, thumbnail::make_preview_thumbnail},
    shared::{
        plugin::PluginCancellation,
        progress::{LogSender, ProgressSender},
        throttle::{CallingThrottle, ThrottleError},
    },
};
use artchiver_sdk::Work;
use rayon::ThreadPool;
use sha2::{Digest as _, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use ureq::Agent;
use crate::plugin::thumbnail::is_image;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("download was cancelled")]
    Cancelled,
    #[error("failed to create temporary file {0}: {1}")]
    TmpFileCreationFailed(PathBuf, #[source] io::Error),
    #[error("failed to rename temporary file {0} -> {1}: {2}")]
    TmpFileRenameFailed(PathBuf, PathBuf, #[source] io::Error),
    #[error("failed to create data directory {0}: {1}")]
    DataDirCreationFailed(PathBuf, #[source] io::Error),
    #[error("failed to start download: {0}")]
    DownloadHeaders(#[from] ureq::Error),
    #[error("failed to download work: {0}")]
    DownloadBody(#[from] io::Error),
    #[error("artchiver is shutting down")]
    Shutdown,
}

pub fn download_works(
    mut works: Vec<Work>,
    db: &DbWriteHandle,
    pool: &ThreadPool,
    (agent, throttle): (&Agent, &CallingThrottle),
    (data_dir, tmp_dir): (&Path, &Path),
    (progress, log, cancellation): (&mut ProgressSender, &mut LogSender, &PluginCancellation),
) -> anyhow::Result<()> {
    log.info(format!("Downloading {} works to disk...", works.len()));
    let works_len = works.len();

    // rayon::scope_fifo(|s| {
    pool.scope_fifo(|s| {
        for (i, work) in works.drain(..).enumerate() {
            let mut progress = progress.clone();
            let mut log = log.clone();

            if cancellation.is_cancelled() {
                return;
            }

            s.spawn_fifo(move |_| {
                progress.set_percent(i, works_len);
                match ensure_work_data_is_cached(
                    &work,
                    db,
                    (agent, throttle),
                    (data_dir, tmp_dir),
                    (&mut log.clone(), cancellation),
                ) {
                    Ok(_) => {}
                    // Note: ignore basic download failures and let the user re-try, if needed.
                    Err(DownloadError::DownloadHeaders(err)) => {
                        log.error(format!(
                            "Error starting download {}: {err}",
                            work.name(),
                            // e.backtrace()
                        ));
                    }
                    Err(DownloadError::DownloadBody(err)) => {
                        log.error(format!(
                            "Error downloading data for {}: {err}",
                            work.name(),
                            // e.backtrace()
                        ));
                    }
                    // Other errors should be fatal and abort all downloads, either because we
                    // requested a Cancellation, or because there is something major wrong.
                    Err(e) => {
                        log.error(format!("Error downloading work {}: {e}", work.name()));
                    }
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
pub fn get_data_path_for_url(data_dir: &Path, url: &str) -> Result<(PathBuf, String), io::Error> {
    let ext = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    let key = Sha256::digest(url.as_bytes());
    let key = format!("{key:x}");
    let level1 = &key[0..2];
    let level2 = &key[2..4];
    let file_base = &key[4..];
    let relative = format!("{level1}/{level2}/{file_base}.{ext}");
    let dir_path = data_dir.join(level1).join(level2);
    fs::create_dir_all(&dir_path)?;
    Ok((data_dir.join(&relative), relative))
}

fn ensure_work_data_is_cached(
    work: &Work,
    db: &DbWriteHandle,
    (agent, throttle): (&Agent, &CallingThrottle),
    (data_dir, tmp_dir): (&Path, &Path),
    (log, cancellation): (&mut LogSender, &PluginCancellation),
) -> Result<(), DownloadError> {
    let mut preview_path = ensure_data_url(
        work.preview_url(),
        data_dir,
        tmp_dir,
        agent,
        log,
        throttle,
        cancellation,
    )?;

    // If the preview we downloaded is not an image, try to thumbnail it.
    if !is_image(&data_dir.join(&preview_path)) {
        match make_preview_thumbnail(work.preview_url(), &preview_path, data_dir, log) {
            Ok(v) => {
                preview_path = v;
            }
            Err(e) => {
                log.warn(format!("failed to make thumbnail: {e}"));
            }
        }
    }

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

    db.set_work_download_paths(work.screen_url(), preview_path, screen_path, archive_path)
        .map_err(|_err| DownloadError::Shutdown)?;
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
) -> Result<String, DownloadError> {
    let (abs_path, rel_path) = get_data_path_for_url(data_dir, url)
        .map_err(|e| DownloadError::DataDirCreationFailed(data_dir.to_path_buf(), e))?;
    if abs_path.exists() {
        // log.trace(format!("cached: ensure_data_url({url})"));
        return Ok(rel_path);
    }

    // Note: check throttle before opening files, etc, but after we might bail for caching.
    match throttle.throttle(cancellation) {
        Ok(_) => {}
        Err(ThrottleError::Cancelled) => return Err(DownloadError::Cancelled),
    };

    let tmp_path = make_temp_path(tmp_dir);
    {
        // Note: in a block to Drop, to close the file before renaming it, just for sanity.
        let tmp_fp = fs::File::create(&tmp_path)
            .map_err(|err| DownloadError::TmpFileCreationFailed(tmp_path.clone(), err))?;
        log.trace(format!("ensure_data_url({url})"));
        let mut resp = agent
            .get(url)
            .call()
            .map_err(DownloadError::DownloadHeaders)?;
        io::copy(
            &mut resp.body_mut().as_reader(),
            &mut io::BufWriter::new(tmp_fp),
        )
        .map_err(DownloadError::DownloadBody)?;
    }
    fs::rename(&tmp_path, &abs_path).map_err(|err| {
        DownloadError::TmpFileRenameFailed(tmp_path.clone(), abs_path.clone(), err)
    })?;
    Ok(rel_path)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_data_path_for_url() {
        let data_dir = PathBuf::from("/tmp/artx/data");
        let (_abs_path, rel_path) =
            get_data_path_for_url(&data_dir, "https://example.com/image.jpg").expect("test");
        assert_eq!(
            rel_path,
            "e5/db/82b5bf63d49d80c5533616892d3386f43955369520986d67653c700fc53c.jpg"
        );
        let (_abs_path, rel_path) =
            get_data_path_for_url(&data_dir, "https://example.com/image.jpg?id=1234")
                .expect("test");
        assert_eq!(
            rel_path,
            "2d/fc/1d0596854b006b8c957f01e07bb0694c77a02cc36efec7bd610ba0409c24.jpg"
        );
    }
}
