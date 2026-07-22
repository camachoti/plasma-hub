use regex::Regex;
use serde::Serialize;
#[cfg(not(target_os = "android"))]
use std::fs;
#[cfg(not(target_os = "android"))]
use std::process::Stdio;
#[cfg(target_os = "android")]
use std::{fs, io::Write, path::PathBuf};
use tauri::{Emitter, Manager};
#[cfg(not(target_os = "android"))]
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(not(target_os = "android"))]
use tokio::process::Command;

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
fn youtube_download_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .download_dir()
        .or_else(|_| {
            app_handle
                .path()
                .app_data_dir()
                .map(|dir| dir.join("downloads"))
        })
        .map_err(|e| e.to_string())
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
pub async fn ensure_ytdlp(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let app_dir = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&app_dir).unwrap_or(());

    #[cfg(target_os = "windows")]
    let binary_name = "yt-dlp.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "yt-dlp";

    let binary_path = app_dir.join(binary_name);

    if !binary_path.exists() {
        #[cfg(target_os = "windows")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
        #[cfg(target_os = "linux")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";
        #[cfg(target_os = "macos")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";

        let response = reqwest::get(download_url)
            .await
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

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn get_youtube_stream_url(
    app_handle: tauri::AppHandle,
    url: String,
    format_id: String,
) -> Result<String, String> {
    let binary_path = ensure_ytdlp(&app_handle).await?;

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best"
    } else {
        &format_id
    };

    let output = std::process::Command::new(&binary_path)
        .arg("-g")
        .arg("-f")
        .arg(format_arg)
        .arg(&url)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let fallback = std::process::Command::new(&binary_path)
            .arg("-g")
            .arg(&url)
            .output()
            .map_err(|e| e.to_string())?;

        if !fallback.status.success() {
            let err = String::from_utf8_lossy(&fallback.stderr);
            return Err(format!("yt-dlp falhou: {}", err));
        }

        let url_str = String::from_utf8_lossy(&fallback.stdout).trim().to_string();
        let first_url = url_str.lines().next().unwrap_or("").to_string();
        return Ok(first_url);
    }

    let url_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let first_url = url_str.lines().next().unwrap_or("").to_string();

    Ok(first_url)
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn get_youtube_stream_url(
    _app_handle: tauri::AppHandle,
    url: String,
    format_id: String,
) -> Result<String, String> {
    get_youtube_direct_stream_url(&url, &format_id).await
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn download_youtube_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    let binary_path = ensure_ytdlp(&app_handle).await?;

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best"
    } else {
        &format_id
    };

    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?;
    let output_path = download_dir.join(&filename);

    let mut child = Command::new(&binary_path)
        .arg("-f")
        .arg(format_arg)
        .arg("-o")
        .arg(output_path)
        .arg("--newline")
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();
    let re = Regex::new(r"\[download\]\s+([\d\.]+)%").unwrap();
    let app_clone = app_handle.clone();
    let id_clone = id.clone();

    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(caps) = re.captures(&line) {
                if let Ok(pct) = caps[1].parse::<f64>() {
                    let _ = app_clone.emit(
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

    let status = child.wait().await.map_err(|e| e.to_string())?;

    if status.success() {
        let _ = app_handle.emit(
            "youtube-download-done",
            DownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    } else {
        let _ = app_handle.emit(
            "youtube-download-error",
            DownloadErrorEvent {
                id,
                error: "Processo de download falhou ou ffmpeg não encontrado".to_string(),
            },
        );
        Err("Download falhou".to_string())
    }
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn download_youtube_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    let result = async {
        let direct_url = get_youtube_direct_stream_url(&url, &format_id).await?;
        let download_dir = youtube_download_dir(&app_handle)?;
        fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
        let output_path = download_dir.join(filename);

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36")
            .build()
            .map_err(|e| e.to_string())?;
        let mut response = client
            .get(direct_url)
            .header("referer", "https://www.youtube.com/")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!("YouTube stream HTTP {}", response.status()));
        }

        let total = response.content_length().unwrap_or(0);
        let mut received = 0_u64;
        let mut file = fs::File::create(output_path).map_err(|e| e.to_string())?;

        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            received += chunk.len() as u64;
            let progress = if total > 0 {
                ((received as f64 / total as f64) * 100.0).min(99.0)
            } else {
                50.0
            };
            let _ = app_handle.emit(
                "youtube-download-progress",
                DownloadProgressEvent {
                    id: id.clone(),
                    progress,
                },
            );
        }

        let _ = app_handle.emit(
            "youtube-download-done",
            DownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    }
    .await;

    if let Err(error) = &result {
        let _ = app_handle.emit(
            "youtube-download-error",
            DownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }

    result
}
