use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::services::download_control::DownloadCancellation;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSource {
    id: String,
    name: String,
    path: Option<String>,
    bytes: Option<Vec<u8>>,
    is_video: bool,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Exact,
    Visual,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    source_id: String,
    path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchProgress {
    run_id: String,
    scanned: u64,
    analyzed: u64,
    matches: Vec<SearchMatch>,
    unreadable: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    scanned: u64,
    analyzed: u64,
    matches: Vec<SearchMatch>,
    unreadable: u64,
    canceled: bool,
}

struct PreparedSource {
    id: String,
    path: Option<PathBuf>,
    bytes: Option<Vec<u8>>,
    size: u64,
    is_video: bool,
}

struct VisualSource {
    id: String,
    aspect: f64,
    duration: Option<f64>,
    hashes: [u64; 2],
}

struct TemporaryVideo(PathBuf);

impl Drop for TemporaryVideo {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn difference_hash(pixels: &[u8]) -> u64 {
    let mut hash = 0;
    for row in 0..8 {
        for col in 0..8 {
            hash = (hash << 1) | u64::from(pixels[row * 9 + col] > pixels[row * 9 + col + 1]);
        }
    }
    hash
}

fn photo_signature_from_image(image: image::DynamicImage) -> (f64, u64) {
    let aspect = image.width() as f64 / image.height() as f64;
    let pixels = image.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    (aspect, difference_hash(pixels.as_raw()))
}

fn photo_signature(path: &Path) -> Result<(f64, u64), String> {
    image::open(path)
        .map(photo_signature_from_image)
        .map_err(|error| error.to_string())
}

fn aspect_close(left: f64, right: f64) -> bool {
    (left / right - 1.0).abs() <= 0.05
}

fn video_info(path: &Path) -> Result<(f64, f64), String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("ffprobe indisponível: {error}"))?;
    if !output.status.success() {
        return Err("Não foi possível ler os dados do vídeo.".into());
    }
    let data: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let stream = &data["streams"][0];
    let width = stream["width"].as_u64().unwrap_or(0);
    let height = stream["height"].as_u64().unwrap_or(0);
    let duration = data["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    if width == 0 || height == 0 || !duration.is_finite() || duration <= 0.0 {
        return Err("Vídeo sem duração ou dimensões válidas.".into());
    }
    Ok((width as f64 / height as f64, duration))
}

fn video_frame_hash(path: &Path, seconds: f64) -> Result<u64, String> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-ss"])
        .arg(format!("{seconds:.3}"))
        .arg("-i")
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=9:8,format=gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .map_err(|error| format!("ffmpeg indisponível: {error}"))?;
    if !output.status.success() || output.stdout.len() != 72 {
        return Err("Não foi possível analisar os quadros do vídeo.".into());
    }
    Ok(difference_hash(&output.stdout))
}

fn video_signature(path: &Path) -> Result<(f64, f64, [u64; 2]), String> {
    let (aspect, duration) = video_info(path)?;
    let hashes = [
        video_frame_hash(path, duration * 0.15)?,
        video_frame_hash(path, duration * 0.60)?,
    ];
    Ok((aspect, duration, hashes))
}

fn is_photo(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "webp"
    )
}

fn is_video(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi"
    )
}

fn temporary_video(bytes: &[u8]) -> Result<TemporaryVideo, String> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("plasma-visual-{}-{id}.mp4", std::process::id()));
    let mut file = File::create_new(&path).map_err(|error| error.to_string())?;
    let temporary = TemporaryVideo(path);
    let written = std::io::Write::write_all(&mut file, bytes);
    drop(file);
    written.map_err(|error| error.to_string())?;
    Ok(temporary)
}

fn prepare_visual_sources(
    sources: &[PreparedSource],
) -> Result<(Vec<VisualSource>, Vec<TemporaryVideo>), String> {
    let mut prepared = Vec::new();
    let mut temporary_files = Vec::new();
    for source in sources {
        if source.is_video {
            let path = match (&source.path, &source.bytes) {
                (Some(path), _) => path.as_path(),
                (_, Some(bytes)) => {
                    temporary_files.push(temporary_video(bytes)?);
                    &temporary_files.last().unwrap().0
                }
                _ => unreachable!(),
            };
            let (aspect, duration, hashes) = video_signature(path)?;
            prepared.push(VisualSource {
                id: source.id.clone(),
                aspect,
                duration: Some(duration),
                hashes,
            });
        } else {
            let (aspect, hash) = match (&source.path, &source.bytes) {
                (Some(path), _) => photo_signature(path)?,
                (_, Some(bytes)) => image::load_from_memory(bytes)
                    .map(photo_signature_from_image)
                    .map_err(|error| error.to_string())?,
                _ => unreachable!(),
            };
            prepared.push(VisualSource {
                id: source.id.clone(),
                aspect,
                duration: None,
                hashes: [hash, 0],
            });
        }
    }
    Ok((prepared, temporary_files))
}

fn same_content(
    source: &PreparedSource,
    candidate: &Path,
    cancellation: &DownloadCancellation,
) -> std::io::Result<bool> {
    let mut candidate = BufReader::new(File::open(candidate)?);
    let source_reader: Box<dyn Read> = match (&source.path, &source.bytes) {
        (Some(path), _) => Box::new(BufReader::new(File::open(path)?)),
        (_, Some(bytes)) => Box::new(std::io::Cursor::new(bytes.as_slice())),
        _ => unreachable!(),
    };
    let mut source_reader = BufReader::new(source_reader);
    let mut left = [0u8; 64 * 1024];
    let mut right = [0u8; 64 * 1024];
    loop {
        if cancellation.is_canceled() {
            return Ok(false);
        }
        let count = source_reader.read(&mut left)?;
        if count == 0 {
            return Ok(true);
        }
        candidate.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
    }
}

fn visual_matches(path: &Path, sources: &[VisualSource]) -> Result<Vec<String>, String> {
    let mut matched = Vec::new();
    if is_photo(path) {
        let dimensions = image::image_dimensions(path).map_err(|error| error.to_string())?;
        let aspect = dimensions.0 as f64 / dimensions.1 as f64;
        let candidates: Vec<_> = sources
            .iter()
            .filter(|source| source.duration.is_none() && aspect_close(source.aspect, aspect))
            .collect();
        if candidates.is_empty() {
            return Ok(matched);
        }
        let (_, hash) = photo_signature(path)?;
        for source in candidates {
            if (source.hashes[0] ^ hash).count_ones() <= 6 {
                matched.push(source.id.clone());
            }
        }
    } else if is_video(path) {
        let (aspect, duration) = video_info(path)?;
        let candidates: Vec<_> = sources
            .iter()
            .filter(|source| {
                source.duration.is_some_and(|source_duration| {
                    aspect_close(source.aspect, aspect)
                        && (source_duration - duration).abs() <= (source_duration * 0.05).max(1.0)
                })
            })
            .collect();
        if candidates.is_empty() {
            return Ok(matched);
        }
        let hashes = [
            video_frame_hash(path, duration * 0.15)?,
            video_frame_hash(path, duration * 0.60)?,
        ];
        for source in candidates {
            if (source.hashes[0] ^ hashes[0]).count_ones() <= 8
                && (source.hashes[1] ^ hashes[1]).count_ones() <= 8
            {
                matched.push(source.id.clone());
            }
        }
    }
    Ok(matched)
}

fn scan<F: FnMut(SearchProgress)>(
    mut on_progress: F,
    run_id: &str,
    folder: &Path,
    sources: &[PreparedSource],
    cancellation: &DownloadCancellation,
    mode: SearchMode,
) -> Result<SearchResult, String> {
    let mut result = SearchResult {
        scanned: 0,
        analyzed: 0,
        matches: Vec::new(),
        unreadable: 0,
        canceled: false,
    };
    let mut sources_by_size: HashMap<u64, Vec<&PreparedSource>> = HashMap::new();
    let (visual_sources, _temporary_files) = if mode == SearchMode::Visual {
        prepare_visual_sources(sources)?
    } else {
        for source in sources {
            sources_by_size.entry(source.size).or_default().push(source);
        }
        (Vec::new(), Vec::new())
    };
    let mut emitted_matches = 0;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if cancellation.is_canceled() {
            result.canceled = true;
            break;
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                result.unreadable += 1;
                continue;
            }
        };
        for entry in entries {
            if cancellation.is_canceled() {
                result.canceled = true;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    result.unreadable += 1;
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => {
                    result.unreadable += 1;
                    continue;
                }
            };
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            // Do not follow symlinks into other trees or count a directory cycle.
            if !kind.is_file() {
                continue;
            }
            result.scanned += 1;
            let previous_matches = result.matches.len();
            if mode == SearchMode::Exact {
                match entry.metadata() {
                    Ok(metadata) => {
                        if let Some(candidates) = sources_by_size.get(&metadata.len()) {
                            for source in candidates {
                                match same_content(source, &path, cancellation) {
                                    Ok(true) => result.matches.push(SearchMatch {
                                        source_id: source.id.clone(),
                                        path: path.to_string_lossy().into_owned(),
                                    }),
                                    Ok(false) => (),
                                    Err(_) => {
                                        result.unreadable += 1;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => result.unreadable += 1,
                }
            } else if (is_photo(&path)
                && visual_sources
                    .iter()
                    .any(|source| source.duration.is_none()))
                || (is_video(&path)
                    && visual_sources
                        .iter()
                        .any(|source| source.duration.is_some()))
            {
                result.analyzed += 1;
                match visual_matches(&path, &visual_sources) {
                    Ok(matches) => {
                        result
                            .matches
                            .extend(matches.into_iter().map(|source_id| SearchMatch {
                                source_id,
                                path: path.to_string_lossy().into_owned(),
                            }))
                    }
                    Err(_) => result.unreadable += 1,
                }
            }
            if result.scanned % 100 == 0 || result.matches.len() > previous_matches {
                on_progress(SearchProgress {
                    run_id: run_id.to_owned(),
                    scanned: result.scanned,
                    analyzed: result.analyzed,
                    matches: result.matches[emitted_matches..].to_vec(),
                    unreadable: result.unreadable,
                });
                emitted_matches = result.matches.len();
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn search_duplicate_media(
    app: AppHandle,
    run_id: String,
    folder_path: String,
    sources: Vec<SearchSource>,
    mode: Option<SearchMode>,
) -> Result<SearchResult, String> {
    let folder = PathBuf::from(folder_path);
    if !folder.is_dir() {
        return Err("Selecione uma pasta válida.".into());
    }
    if sources.is_empty() {
        return Err("Nenhuma mídia foi selecionada.".into());
    }
    let prepared = sources
        .into_iter()
        .map(|source| {
            if source.id.is_empty() || source.name.is_empty() {
                return Err("Mídia inválida.".to_string());
            }
            match (source.path, source.bytes) {
                (Some(path), None) => {
                    let path = PathBuf::from(path);
                    let size = path.metadata().map_err(|error| error.to_string())?.len();
                    if !path.is_file() {
                        return Err("Arquivo da mídia indisponível.".into());
                    }
                    Ok(PreparedSource {
                        id: source.id,
                        path: Some(path),
                        bytes: None,
                        size,
                        is_video: source.is_video,
                    })
                }
                (None, Some(bytes)) => Ok(PreparedSource {
                    id: source.id,
                    path: None,
                    size: bytes.len() as u64,
                    bytes: Some(bytes),
                    is_video: source.is_video,
                }),
                _ => Err("A mídia precisa de um arquivo ou de seus dados.".into()),
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    tokio::task::spawn_blocking(move || {
        let cancellation = DownloadCancellation::register(format!("duplicate-search-{run_id}"));
        scan(
            |progress| {
                let _ = app.emit("duplicate-media-search-progress", progress);
            },
            &run_id,
            &folder,
            &prepared,
            &cancellation,
            mode.unwrap_or(SearchMode::Exact),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_identical_content_in_subfolders_even_with_another_name() {
        let root =
            std::env::temp_dir().join(format!("plasma-duplicate-search-{}", std::process::id()));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("same-size.jpg"), b"different").unwrap();
        fs::write(nested.join("renamed.png"), b"identical").unwrap();

        let source = PreparedSource {
            id: "image-1".into(),
            path: None,
            bytes: Some(b"identical".to_vec()),
            size: 9,
            is_video: false,
        };
        let cancellation = DownloadCancellation::register("duplicate-search-test");
        let result = scan(
            |_| {},
            "test",
            &root,
            &[source],
            &cancellation,
            SearchMode::Exact,
        )
        .unwrap();
        assert_eq!(result.scanned, 2);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(
            result.matches[0].path,
            nested.join("renamed.png").to_string_lossy()
        );
        assert!(!result.canceled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finds_resized_recompressed_photo() {
        let root = std::env::temp_dir().join(format!("plasma-visual-photo-{}", std::process::id()));
        let folder = root.join("folder");
        fs::create_dir_all(&folder).unwrap();
        let original = root.join("original.png");
        let photo = image::RgbImage::from_fn(160, 96, |x, y| {
            image::Rgb([
                (x / 4) as u8 * 5,
                (y / 3) as u8 * 6,
                ((x / 16 + y / 12) % 2) as u8 * 180,
            ])
        });
        photo.save(&original).unwrap();
        image::imageops::resize(&photo, 80, 48, FilterType::Triangle)
            .save(folder.join("smaller.jpg"))
            .unwrap();
        image::RgbImage::from_fn(160, 96, |x, _| {
            image::Rgb([if x % 32 < 16 { 0 } else { 255 }, 0, 0])
        })
        .save(folder.join("other.png"))
        .unwrap();
        let source = PreparedSource {
            id: "photo".into(),
            path: Some(original.clone()),
            bytes: None,
            size: original.metadata().unwrap().len(),
            is_video: false,
        };
        let cancellation = DownloadCancellation::register("visual-photo-test");
        let result = scan(
            |_| {},
            "test",
            &folder,
            &[source],
            &cancellation,
            SearchMode::Visual,
        )
        .unwrap();
        assert_eq!(result.analyzed, 2);
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].path.ends_with("smaller.jpg"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finds_reencoded_video() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("plasma-visual-video-{}", std::process::id()));
        let folder = root.join("folder");
        fs::create_dir_all(&folder).unwrap();
        let original = root.join("original.mp4");
        let reencoded = folder.join("reencoded.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=10:duration=2",
                "-c:v",
                "mpeg4",
                "-q:v",
                "3",
                "-y",
            ])
            .arg(&original)
            .status()
            .unwrap();
        assert!(generated.success());
        let converted = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&original)
            .args(["-c:v", "mpeg4", "-q:v", "12", "-y"])
            .arg(&reencoded)
            .status()
            .unwrap();
        assert!(converted.success());
        let source = PreparedSource {
            id: "video".into(),
            path: Some(original.clone()),
            bytes: None,
            size: original.metadata().unwrap().len(),
            is_video: true,
        };
        let cancellation = DownloadCancellation::register("visual-video-test");
        let result = scan(
            |_| {},
            "test",
            &folder,
            &[source],
            &cancellation,
            SearchMode::Visual,
        )
        .unwrap();
        assert_eq!(result.analyzed, 1);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].path, reencoded.to_string_lossy());
        fs::remove_dir_all(root).unwrap();
    }
}
