//! Media cache helpers shared by the Telegram and download domains.

use plasma_core::AppEnv;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "android"))]
use std::process::Command;

/// Extracts a JPEG poster for a local video and caches it in the app cache
/// directory. Mirrors the previous `generate_video_thumbnail` command.
pub fn generate_video_thumbnail(app: &AppEnv, file_path: &str) -> Result<String, String> {
    let source_path = Path::new(file_path);
    if !source_path.is_file() {
        return Err("Arquivo de vídeo não encontrado.".to_string());
    }

    let metadata = std::fs::metadata(source_path).map_err(|error| error.to_string())?;
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    if let Ok(modified) = metadata.modified() {
        modified.hash(&mut hasher);
    }

    let directory = app.paths().cache_dir().join("drop-video-thumbnails");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;

    let output_path = directory.join(format!("{:x}.jpg", hasher.finish()));
    if output_path.is_file() {
        return Ok(output_path.to_string_lossy().to_string());
    }

    generate_video_thumbnail_to_path(source_path, &output_path)?;
    Ok(output_path.to_string_lossy().to_string())
}

/// Extracts a JPEG poster into a specific path.
///
/// Desktop uses `ffmpeg`; Android uses `MediaMetadataRetriever` through JNI so
/// the same cache layout works on both platforms.
pub fn generate_video_thumbnail_to_path(
    source_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    if !source_path.is_file() {
        return Err("Arquivo de vídeo não encontrado.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        return plasma_platform::android_video_thumbnail(
            source_path.to_string_lossy().as_ref(),
            output_path,
        );
    }

    #[cfg(not(target_os = "android"))]
    {
        let source = source_path.to_string_lossy().to_string();
        let output = output_path.to_string_lossy().to_string();
        let attempts = ["1", "0.25", "0"];
        let mut last_error = String::new();

        for seek in attempts {
            let status = Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-ss",
                    seek,
                    "-i",
                    &source,
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=420:-2",
                    &output,
                ])
                .status();

            match status {
                Ok(exit_status) if exit_status.success() && output_path.is_file() => {
                    return Ok(());
                }
                Ok(exit_status) => {
                    last_error = format!("ffmpeg saiu com status {exit_status}");
                    let _ = std::fs::remove_file(output_path);
                }
                Err(error) => {
                    last_error = if error.kind() == std::io::ErrorKind::NotFound {
                        "ffmpeg não encontrado no sistema.".to_string()
                    } else {
                        error.to_string()
                    };
                    break;
                }
            }
        }

        Err(last_error)
    }
}

/// Writes a JPEG the WebKit shell can paint for a still image it cannot decode.
///
/// Desktop tries `heif-convert` and then `ffmpeg`. Android WebView can decode
/// HEIC itself, so this stays a desktop helper.
pub fn jpeg_preview_for_still(source: &Path, output: &Path) -> Result<(), String> {
    if !source.is_file() {
        return Err("Arquivo de imagem não encontrado.".to_string());
    }
    if output.is_file() {
        return Ok(());
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "android")]
    {
        let _ = (source, output);
        return Err("Prévia HEIC fica a cargo do WebView no Android.".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        let source_arg = source.to_string_lossy().to_string();
        let output_arg = output.to_string_lossy().to_string();
        let heif = Command::new("heif-convert")
            .args([&source_arg, &output_arg])
            .status();
        if matches!(&heif, Ok(status) if status.success() && output.is_file()) {
            return Ok(());
        }
        let _ = std::fs::remove_file(output);

        let ffmpeg = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                &source_arg,
                "-frames:v",
                "1",
                &output_arg,
            ])
            .status();
        match ffmpeg {
            Ok(status) if status.success() && output.is_file() => Ok(()),
            Ok(status) => {
                let _ = std::fs::remove_file(output);
                Err(format!("ffmpeg saiu com status {status}"))
            }
            Err(error) => Err(if error.kind() == std::io::ErrorKind::NotFound {
                "nem heif-convert nem ffmpeg estão disponíveis.".to_string()
            } else {
                error.to_string()
            }),
        }
    }
}

/// QuickTime containers Telegram will not stream as an inline video.
pub fn is_quicktime_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "mov" || extension == "qt"
        })
        .unwrap_or(false)
}

/// Returns an MP4 Telegram can send as a video. Other files pass through.
///
/// The original is left in place. A repeated send of the same bytes reuses the
/// cached MP4. Desktop remuxes with `+faststart` and re-encodes to H.264 when
/// the video codec is not one Telegram streams. Android keeps the original.
pub fn prepare_quicktime_for_send(cache_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    if !is_quicktime_path(source) {
        return Ok(source.to_path_buf());
    }
    if !source.is_file() {
        return Err("Arquivo de vídeo não encontrado.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        let _ = cache_dir;
        return Ok(source.to_path_buf());
    }

    #[cfg(not(target_os = "android"))]
    {
        let output = quicktime_cache_path(cache_dir, source);
        if output.is_file() {
            return Ok(output);
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        quicktime_to_mp4(source, &output)?;
        Ok(output)
    }
}

#[cfg(not(target_os = "android"))]
fn quicktime_cache_path(cache_dir: &Path, source: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    if let Ok(metadata) = std::fs::metadata(source) {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified() {
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default()
                .hash(&mut hasher);
        }
    }
    cache_dir
        .join("outbound-video")
        .join(format!("{:x}.mp4", hasher.finish()))
}

#[cfg(not(target_os = "android"))]
fn quicktime_to_mp4(source: &Path, output: &Path) -> Result<(), String> {
    let partial = output.with_extension("part");
    let _ = std::fs::remove_file(&partial);
    let remuxed = ffmpeg_to(
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            &source.to_string_lossy(),
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            &partial.to_string_lossy(),
        ],
        &partial,
    );
    let keep_remux = match &remuxed {
        Ok(()) => video_codec_streams_inline(&partial),
        Err(error) if ffmpeg_missing(error) => return Err(error.clone()),
        Err(_) => false,
    };
    if keep_remux {
        return std::fs::rename(&partial, output).map_err(|error| error.to_string());
    }
    let _ = std::fs::remove_file(&partial);
    ffmpeg_to(
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            &source.to_string_lossy(),
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "+faststart",
            &partial.to_string_lossy(),
        ],
        &partial,
    )?;
    std::fs::rename(&partial, output).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "android"))]
fn ffmpeg_missing(error: &str) -> bool {
    error.contains("ffmpeg não encontrado")
}

#[cfg(not(target_os = "android"))]
fn ffmpeg_to(args: &[&str], output: &Path) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args(args)
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "ffmpeg não encontrado no sistema.".to_string()
            } else {
                error.to_string()
            }
        })?;
    if status.success()
        && output.is_file()
        && std::fs::metadata(output)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false)
    {
        Ok(())
    } else {
        let _ = std::fs::remove_file(output);
        Err(format!("ffmpeg saiu com status {status}"))
    }
}

/// H.264 and MPEG-4 are what Telegram streams inline. Missing `ffprobe` keeps
/// the remux instead of forcing a long re-encode.
#[cfg(not(target_os = "android"))]
fn video_codec_streams_inline(path: &Path) -> bool {
    let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
            &path.to_string_lossy(),
        ])
        .output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    let codec = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    codec.is_empty() || matches!(codec.as_str(), "h264" | "mpeg4")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp4_is_not_quicktime() {
        assert!(!is_quicktime_path(Path::new("/tmp/clip.mp4")));
        assert!(!is_quicktime_path(Path::new("/tmp/clip.MP4")));
        assert!(is_quicktime_path(Path::new("/tmp/clip.MOV")));
        assert!(is_quicktime_path(Path::new("/tmp/clip.qt")));
    }

    #[test]
    fn other_files_skip_conversion() {
        let source = Path::new("/tmp/clip.mp4");
        let prepared = prepare_quicktime_for_send(Path::new("/tmp/cache"), source).unwrap();
        assert_eq!(prepared, source);
    }
}
