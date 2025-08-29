use crate::shared::progress::LogSender;
use anyhow::Result;
use std::path::Path;

pub fn is_image(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    const IMAGE_EXTENSIONS: &[&str] = &[
        "avif", "bmp", "dds", "exr", "ff", "gif", "hdr", "ico", "jpeg", "jpg", "png", "pnm", "qoi",
        "tga", "tiff", "tif", "webp",
    ];
    IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().to_str().unwrap_or_default())
}

pub fn is_audio(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    const AUDIO_EXTENSIONS: &[&str] = &["aac", "flac", "m4a", "mp3", "ogg", "wav", "wma"];
    AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().to_str().unwrap_or_default())
}

pub fn is_archive(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    const ARCHIVE_EXTENSIONS: &[&str] = &[
        "7z", "ar", "bz2", "cab", "cpio", "deb", "gz", "iso", "jar", "rar", "rpm", "tar", "xz",
        "z", "zip",
    ];
    ARCHIVE_EXTENSIONS.contains(&ext.to_ascii_lowercase().to_str().unwrap_or_default())
}

pub fn is_pdf(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    const PDF_EXTENSIONS: &[&str] = &["pdf"];
    PDF_EXTENSIONS.contains(&ext.to_ascii_lowercase().to_str().unwrap_or_default())
}

// If the plugin gives us back a preview path that is not an image -- e.g. a downsampled full video,
// or an audio podcast sample -- try to get a preview image somehow. The input here is the url and
// the storge path components. The output needs to be a new path prefix relative to the data dir.
// Typically, this will be obtained by adding something to the preview_url and calling back into
// get_data_path_for_url to hash the new URL. The DbWork will store the new path we return so that
// subsequent usage will see the file we generated instead of the plugin's preview file, but that
// file can still be found via the preview URL stored in the DbWork.
//
// Note that failure is absolutely an option here. If we can't figure out how to make a preview
// image, just return the original preview path.
pub fn make_preview_thumbnail(
    preview_url: &str,
    rel_path: &str,
    data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    log.trace(format!("make_preview_thumbnail({rel_path})"));
    let abs_path = data_dir.join(rel_path);
    if is_image(&abs_path) {
        return Ok(rel_path.to_owned());
    }

    if is_image(&abs_path) {
        make_image_preview_image(preview_url, &abs_path, rel_path, data_dir, log)
    } else if is_audio(&abs_path) {
        make_audio_preview_image(preview_url, &abs_path, rel_path, data_dir, log)
    } else if is_archive(&abs_path) {
        make_archive_preview_image(preview_url, &abs_path, rel_path, data_dir, log)
    } else if is_pdf(&abs_path) {
        make_pdf_preview_image(preview_url, &abs_path, rel_path, data_dir, log)
    } else {
        make_video_preview_image(preview_url, &abs_path, rel_path, data_dir, log)
    }
}

#[expect(clippy::unnecessary_wraps)]
fn make_image_preview_image(
    _preview_url: &str,
    _abs_path: &Path,
    rel_path: &str,
    _data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    log.error("TODO: make a preview image for a full size image");
    Ok(rel_path.to_owned())
}

#[expect(clippy::unnecessary_wraps)]
fn make_audio_preview_image(
    _preview_url: &str,
    _abs_path: &Path,
    rel_path: &str,
    _data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    // We need to do the equivalent of:
    //     ffmpeg -i in.flac -filter_complex "showwavespic=s=640x320:colors=black" -frames:v 1 out.png
    log.error("TODO: make a preview image for an audio file");
    Ok(rel_path.to_owned())
}

#[expect(clippy::unnecessary_wraps)]
fn make_archive_preview_image(
    _preview_url: &str,
    _abs_path: &Path,
    rel_path: &str,
    _data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    // Look for the first image file
    log.error("TODO: make a preview image for an archive file");
    Ok(rel_path.to_owned())
}

#[expect(clippy::unnecessary_wraps)]
fn make_pdf_preview_image(
    _preview_url: &str,
    _abs_path: &Path,
    rel_path: &str,
    _data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    // Print the first page to an image
    log.error("TODO: make a preview image for a PDF file");
    Ok(rel_path.to_owned())
}

#[expect(clippy::unnecessary_wraps)]
fn make_video_preview_image(
    _preview_url: &str,
    _abs_path: &Path,
    rel_path: &str,
    _data_dir: &Path,
    log: &mut LogSender,
) -> Result<String> {
    // Get a random frame from a few seconds into the video, or the first frame if the video is too short.
    log.error("TODO: make a preview image for a video file");
    Ok(rel_path.to_owned())
}
