use plasma_core::AppEnv;
use regex::Regex;
use serde::Serialize;
#[cfg(not(target_os = "android"))]
use std::fs;
#[cfg(not(target_os = "android"))]
use std::process::Stdio;
#[cfg(target_os = "android")]
use std::{fs, io::Write, path::PathBuf};
#[cfg(not(target_os = "android"))]
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(not(target_os = "android"))]
use tokio::process::Command;

#[cfg(target_os = "android")]
use plasma_download::is_cancellation_error;
use plasma_download::{DownloadCancellation, DOWNLOAD_CANCELED_ERROR};

#[derive(Clone, Serialize)]
struct DownloadProgressEvent {
    id: String,
    progress: f64,
}

#[derive(Clone, Serialize)]
struct DownloadErrorEvent {
    id: String,
    error: String,
}

#[derive(Clone, Serialize)]
struct DownloadDoneEvent {
    id: String,
}

#[cfg(target_os = "android")]
fn youtube_download_dir(app_handle: &AppEnv) -> Result<PathBuf, String> {
    app_handle
        .paths()
        .download_dir()
        .or_else(|_| Ok(app_handle.paths().data_dir().join("downloads")))
}

#[cfg(target_os = "android")]
fn youtube_json_from_html(html: &str) -> Result<&str, String> {
    let start = html
        .find("ytInitialPlayerResponse = ")
        .ok_or_else(|| "Nao foi possivel extrair dados do YouTube.".to_string())?
        + "ytInitialPlayerResponse = ".len();
    let rest = &html[start..];
    let end = rest
        .find(";</script>")
        .or_else(|| rest.find("};var ").map(|idx| idx + 1))
        .ok_or_else(|| "Nao foi possivel localizar fim do JSON do YouTube.".to_string())?;
    Ok(&rest[..end])
}

#[cfg(target_os = "android")]
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                output.push(hex);
                i += 3;
                continue;
            }
        }
        output.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

#[cfg(target_os = "android")]
fn query_param(input: &str, key: &str) -> Option<String> {
    input.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        let pair_key = parts.next()?;
        let pair_value = parts.next().unwrap_or_default();
        if pair_key == key {
            Some(percent_decode(pair_value))
        } else {
            None
        }
    })
}

#[cfg(target_os = "android")]
fn absolute_youtube_url(path: &str) -> String {
    if path.starts_with("http") {
        path.to_string()
    } else if path.starts_with("//") {
        format!("https:{}", path)
    } else {
        format!("https://www.youtube.com{}", path)
    }
}

#[cfg(target_os = "android")]
fn player_js_url_from_html(html: &str) -> Option<String> {
    let patterns = [
        r#""jsUrl":"([^"]+)""#,
        r#""PLAYER_JS_URL":"([^"]+)""#,
        r#""js":"([^"]+/base\.js)""#,
    ];

    patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(html)
            .and_then(|captures| {
                captures
                    .get(1)
                    .map(|value| absolute_youtube_url(value.as_str()))
            })
    })
}

#[cfg(target_os = "android")]
fn extract_balanced_function_body(js: &str, start_brace: usize) -> Option<&str> {
    let mut depth = 0_i32;
    let mut body_start = None;
    for (offset, ch) in js[start_brace..].char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if body_start.is_none() {
                    body_start = Some(start_brace + offset + ch.len_utf8());
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body_start.map(|start| &js[start..start_brace + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(target_os = "android")]
fn decipher_function_body<'a>(js: &'a str, name: &str) -> Option<&'a str> {
    let escaped = regex::escape(name);
    let patterns = [
        format!(r#"function\s+{}\s*\(\w+\)\s*\{{"#, escaped),
        format!(r#"{}\s*=\s*function\s*\(\w+\)\s*\{{"#, escaped),
        format!(r#"var\s+{}\s*=\s*function\s*\(\w+\)\s*\{{"#, escaped),
    ];

    patterns.iter().find_map(|pattern| {
        let mat = Regex::new(pattern).ok()?.find(js)?;
        extract_balanced_function_body(js, mat.end() - 1)
    })
}

#[cfg(target_os = "android")]
fn decipher_function_name(js: &str) -> Option<String> {
    let patterns = [
        r#"\.sig\|\|([A-Za-z0-9_$]+)\("#,
        r#"signature",([A-Za-z0-9_$]+)\("#,
        r#"set\("signature",([A-Za-z0-9_$]+)\("#,
    ];

    patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(js)
            .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
    })
}

#[cfg(target_os = "android")]
fn helper_object_body<'a>(js: &'a str, helper_name: &str) -> Option<&'a str> {
    let pattern = format!(r#"(?:var\s+)?{}\s*=\s*\{{"#, regex::escape(helper_name));
    let mat = Regex::new(&pattern).ok()?.find(js)?;
    extract_balanced_function_body(js, mat.end() - 1)
}

#[cfg(target_os = "android")]
fn helper_method_body<'a>(object_body: &'a str, method_name: &str) -> Option<&'a str> {
    let escaped = regex::escape(method_name);
    let patterns = [
        format!(r#"{}\s*:\s*function\s*\([^)]*\)\s*\{{"#, escaped),
        format!(r#""{}"\s*:\s*function\s*\([^)]*\)\s*\{{"#, escaped),
        format!(r#"'{}'\s*:\s*function\s*\([^)]*\)\s*\{{"#, escaped),
    ];

    patterns.iter().find_map(|pattern| {
        let mat = Regex::new(pattern).ok()?.find(object_body)?;
        extract_balanced_function_body(object_body, mat.end() - 1)
    })
}

#[cfg(target_os = "android")]
fn decipher_signature(signature: &str, player_js: &str) -> Result<String, String> {
    let function_name = decipher_function_name(player_js)
        .ok_or_else(|| "Nao foi possivel localizar funcao de assinatura do YouTube.".to_string())?;
    let function_body = decipher_function_body(player_js, &function_name)
        .ok_or_else(|| "Nao foi possivel ler funcao de assinatura do YouTube.".to_string())?;
    let helper_name = Regex::new(r#"([A-Za-z0-9_$]+)\.([A-Za-z0-9_$]+)\(\w+,(\d+)\)"#)
        .map_err(|e| e.to_string())?
        .captures(function_body)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .ok_or_else(|| "Nao foi possivel localizar helper de assinatura do YouTube.".to_string())?;
    let object_body = helper_object_body(player_js, &helper_name)
        .ok_or_else(|| "Nao foi possivel ler helper de assinatura do YouTube.".to_string())?;

    let op_re = Regex::new(&format!(
        r#"{}\.(?P<method>[A-Za-z0-9_$]+)\(\w+,(?P<arg>\d+)\)"#,
        regex::escape(&helper_name)
    ))
    .map_err(|e| e.to_string())?;

    let mut chars: Vec<char> = signature.chars().collect();
    for captures in op_re.captures_iter(function_body) {
        let method = captures
            .name("method")
            .map(|value| value.as_str())
            .unwrap_or_default();
        let arg = captures
            .name("arg")
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or_default();
        let method_body = helper_method_body(object_body, method)
            .ok_or_else(|| "Operacao de assinatura do YouTube desconhecida.".to_string())?;

        if method_body.contains(".reverse(") {
            chars.reverse();
        } else if method_body.contains(".splice(") {
            let drain_to = arg.min(chars.len());
            chars.drain(0..drain_to);
        } else if method_body.contains("%") || method_body.contains("[0]") {
            if !chars.is_empty() {
                let index = arg % chars.len();
                chars.swap(0, index);
            }
        } else {
            return Err("Operacao de assinatura do YouTube nao suportada.".to_string());
        }
    }

    Ok(chars.into_iter().collect())
}

#[cfg(target_os = "android")]
async fn format_direct_url(
    format: &serde_json::Value,
    player_js: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(url) = format.get("url").and_then(|value| value.as_str()) {
        return Ok(Some(url.to_string()));
    }

    let cipher = format
        .get("signatureCipher")
        .or_else(|| format.get("cipher"))
        .and_then(|value| value.as_str());
    let Some(cipher) = cipher else {
        return Ok(None);
    };

    let base_url =
        query_param(cipher, "url").ok_or_else(|| "Cipher do YouTube sem URL base.".to_string())?;
    let encrypted_signature =
        query_param(cipher, "s").ok_or_else(|| "Cipher do YouTube sem assinatura.".to_string())?;
    let signature_param = query_param(cipher, "sp").unwrap_or_else(|| "signature".to_string());
    let player_js = player_js
        .ok_or_else(|| "Player JS do YouTube indisponivel para decifrar assinatura.".to_string())?;
    let signature = decipher_signature(&encrypted_signature, player_js)?;
    let separator = if base_url.contains('?') { '&' } else { '?' };
    Ok(Some(format!(
        "{}{}{}={}",
        base_url, separator, signature_param, signature
    )))
}

#[cfg(target_os = "android")]
async fn get_youtube_direct_stream_url(url: &str, format_id: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36",
        )
        .build()
        .map_err(|e| e.to_string())?;
    let html = client
        .get(url)
        .header("accept-language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let data: serde_json::Value =
        serde_json::from_str(youtube_json_from_html(&html)?).map_err(|e| e.to_string())?;
    let player_js_url = player_js_url_from_html(&html);
    let player_js = if let Some(player_js_url) = player_js_url {
        Some(
            client
                .get(player_js_url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let target_itag = format_id
        .split('+')
        .next()
        .and_then(|value| value.parse::<i64>().ok());

    let formats = data
        .pointer("/streamingData/formats")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .chain(
            data.pointer("/streamingData/adaptiveFormats")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten(),
        );

    let mut first_direct_url = None;
    for format in formats {
        let direct_url = format_direct_url(format, player_js.as_deref()).await?;
        if first_direct_url.is_none() {
            first_direct_url = direct_url.clone();
        }

        if let Some(target_itag) = target_itag {
            if format.get("itag").and_then(|value| value.as_i64()) == Some(target_itag) {
                return direct_url.ok_or_else(|| {
                    "Formato do YouTube sem URL resolvivel no Android.".to_string()
                });
            }
        }
    }

    first_direct_url
        .ok_or_else(|| "Nenhum stream resolvivel do YouTube encontrado no Android.".to_string())
}

#[cfg(not(target_os = "android"))]
pub async fn ensure_ytdlp(app_handle: &AppEnv) -> Result<std::path::PathBuf, String> {
    let app_dir = app_handle.paths().local_data_dir().to_path_buf();
    fs::create_dir_all(&app_dir).unwrap_or(());

    #[cfg(target_os = "windows")]
    let binary_name = "yt-dlp.exe";
    #[cfg(target_os = "linux")]
    let binary_name = "yt-dlp_linux";
    #[cfg(target_os = "macos")]
    let binary_name = "yt-dlp";

    let binary_path = app_dir.join(binary_name);

    if !binary_path.exists() {
        #[cfg(target_os = "windows")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        let download_url =
            "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64";
        #[cfg(all(
            target_os = "linux",
            not(any(target_arch = "x86_64", target_arch = "aarch64"))
        ))]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";
        #[cfg(target_os = "macos")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";

        let response = reqwest::get(download_url)
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        fs::write(&binary_path, bytes).map_err(|e| e.to_string())?;

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&binary_path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&binary_path, perms).map_err(|e| e.to_string())?;
        }
    }

    Ok(binary_path)
}

#[cfg(not(target_os = "android"))]
pub async fn get_youtube_stream_url(
    app_handle: &AppEnv,
    url: String,
    format_id: String,
) -> Result<String, String> {
    let cache_dir = app_handle.paths().cache_dir().join("ytdlp");
    let _ = fs::create_dir_all(&cache_dir);

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best"
    } else {
        &format_id
    };

    #[cfg(target_os = "linux")]
    let fast_runner: Option<(std::path::PathBuf, std::path::PathBuf)> =
        ensure_ytdlp_zipapp(app_handle).await.ok();
    #[cfg(not(target_os = "linux"))]
    let fast_runner: Option<(std::path::PathBuf, std::path::PathBuf)> = None;

    let binary_path = if fast_runner.is_none() {
        Some(ensure_ytdlp(app_handle).await?)
    } else {
        None
    };

    let build_cmd = |fmt: &str| {
        let mut cmd = if let Some((ref py, ref script)) = fast_runner {
            let mut c = Command::new(py);
            c.arg(script);
            c
        } else {
            Command::new(binary_path.as_ref().expect("binary_path"))
        };
        cmd.arg("--no-playlist")
            .arg("--no-warnings")
            .arg("--cache-dir")
            .arg(&cache_dir)
            .arg("--socket-timeout")
            .arg("6")
            .arg("-g")
            .arg("-f")
            .arg(fmt)
            .arg(&url);
        cmd
    };

    let primary = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        build_cmd(format_arg).output(),
    )
    .await
    .map_err(|_| "Tempo esgotado ao resolver stream no yt-dlp.".to_string())?
    .map_err(|e| e.to_string())?;

    if primary.status.success() {
        let url_str = String::from_utf8_lossy(&primary.stdout).trim().to_string();
        if url_str.lines().any(|l| l.starts_with("https://")) {
            return Ok(url_str);
        }
    }

    let err = String::from_utf8_lossy(&primary.stderr);
    Err(format!("yt-dlp falhou: {}", err))
}

#[cfg(target_os = "linux")]
async fn ensure_ytdlp_zipapp(
    app_handle: &AppEnv,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let py = std::path::PathBuf::from("/usr/bin/python3");
    if !py.exists() {
        return Err("python3 indisponível".to_string());
    }
    let app_dir = app_handle.paths().local_data_dir().to_path_buf();
    let _ = fs::create_dir_all(&app_dir);
    let zip_path = app_dir.join("yt-dlp_zip");
    if !zip_path.exists() {
        let resp = reqwest::get("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp")
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        fs::write(&zip_path, bytes).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&zip_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&zip_path, perms);
        }
    }
    Ok((py, zip_path))
}

#[cfg(target_os = "android")]
pub async fn get_youtube_stream_url(
    _app_handle: &AppEnv,
    url: String,
    format_id: String,
) -> Result<String, String> {
    get_youtube_direct_stream_url(&url, &format_id).await
}

#[cfg(not(target_os = "android"))]
pub async fn download_youtube_native(
    app_handle: &AppEnv,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let binary_path = ensure_ytdlp(app_handle).await?;

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best".to_string()
    } else {
        format_id
    };

    let download_dir = app_handle.paths().download_dir()?;
    let output_path = download_dir.join(&filename);
    let info_path = output_path.with_extension("plasma-info.json");
    prepare_youtube_info(&binary_path, &url, &format_arg, &info_path).await?;

    let mut child = Command::new(&binary_path)
        .arg("--load-info-json")
        .arg(&info_path)
        .arg("-f")
        .arg(&format_arg)
        .arg("-o")
        .arg(&output_path)
        .arg("--continue")
        .arg("--no-clean-info-json")
        .arg("--newline")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();
    let re = Regex::new(r"\[download\]\s+([\d\.]+)%").unwrap();
    let app_clone = (*app_handle).clone();
    let id_clone = id.clone();

    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(caps) = re.captures(&line) {
                if let Ok(pct) = caps[1].parse::<f64>() {
                    app_clone.emit(
                        "youtube-download-progress",
                        DownloadProgressEvent {
                            id: id_clone.clone(),
                            progress: pct,
                        },
                    );
                }
            }
        }
    });

    if cancellation.is_canceled() {
        let _ = child.kill().await;
        return Err(DOWNLOAD_CANCELED_ERROR.to_string());
    }

    let status = tokio::select! {
        status = child.wait() => status.map_err(|e| e.to_string())?,
        _ = cancellation.wait() => {
            let _ = child.kill().await;
            return Err(DOWNLOAD_CANCELED_ERROR.to_string());
        }
    };

    if status.success() {
        app_handle.emit(
            "youtube-download-done",
            DownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    } else {
        app_handle.emit(
            "youtube-download-error",
            DownloadErrorEvent {
                id,
                error: "Processo de download falhou ou ffmpeg não encontrado".to_string(),
            },
        );
        Err("Download falhou".to_string())
    }
}

/// Saves the chosen format once. A later retry loads this file and continues
/// the same `.part` instead of opening the watch page again.
#[cfg(not(target_os = "android"))]
async fn prepare_youtube_info(
    binary: &std::path::Path,
    url: &str,
    format: &str,
    info_path: &std::path::Path,
) -> Result<(), String> {
    if info_path.is_file() {
        if let Ok(metadata) = fs::metadata(info_path) {
            if metadata.len() > 2 {
                return Ok(());
            }
        }
    }
    let output = Command::new(binary)
        .arg("--no-playlist")
        .arg("-J")
        .arg("-f")
        .arg(format)
        .arg(url)
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp falhou: {error}"));
    }
    if let Some(parent) = info_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(info_path, &output.stdout).map_err(|error| error.to_string())
}

#[cfg(target_os = "android")]
pub async fn download_youtube_native(
    app_handle: &AppEnv,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let result = async {
        let direct_url = tokio::select! {
            result = get_youtube_direct_stream_url(&url, &format_id) => result?,
            _ = cancellation.wait() => return Err(DOWNLOAD_CANCELED_ERROR.to_string()),
        };
        drop(cancellation);
        let download_dir = youtube_download_dir(app_handle)?;
        let output_path = download_dir.join(filename);
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "referer".to_string(),
            "https://www.youtube.com/".to_string(),
        );
        headers.insert(
            "user-agent".to_string(),
            "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36"
                .to_string(),
        );
        plasma_download::download_url_to_file(
            app_handle,
            &id,
            &direct_url,
            &output_path.to_string_lossy(),
            Some(headers),
        )
        .await?;

        app_handle.emit(
            "youtube-download-done",
            DownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    }
    .await;

    if let Err(error) = &result {
        if is_cancellation_error(error) {
            return result;
        }
        app_handle.emit(
            "youtube-download-error",
            DownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }

    result
}
