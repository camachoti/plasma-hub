//! Media cache helpers shared by the Telegram and download domains.

use plasma_core::AppEnv;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

/// Extracts a JPEG poster for a local video using `ffmpeg` and caches it in the
/// app cache directory. Mirrors the previous `generate_video_thumbnail` command.
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
                return Ok(output);
            }
            Ok(exit_status) => {
                last_error = format!("ffmpeg saiu com status {exit_status}");
                let _ = std::fs::remove_file(&output_path);
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
