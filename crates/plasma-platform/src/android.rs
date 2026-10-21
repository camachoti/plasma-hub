//! Android platform integration through JNI.
//!
//! The Dioxus Android runtime initializes `ndk-context` before it invokes the
//! Rust `main`, so paths, intents and media helpers can use the live Activity
//! without depending on `tauri` or `dioxus`.
#![allow(unsafe_code)]

use crate::ShareHandler;
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use ndk_context::android_context;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::sync::oneshot;

/// Filesystem locations resolved from the Android application context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidAppDirs {
    pub data_dir: PathBuf,
    pub local_data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub download_dir: PathBuf,
}

static SHARE_HANDLER: Mutex<Option<ShareHandler>> = Mutex::new(None);
static PENDING_SHARED_URL: Mutex<Option<String>> = Mutex::new(None);
static PICK_TX: Mutex<Option<oneshot::Sender<Option<PathBuf>>>> = Mutex::new(None);

fn with_env<R>(
    handler: impl for<'local> FnOnce(&mut JNIEnv<'local>, &JObject<'local>) -> Result<R, String>,
) -> Result<R, String> {
    let context = android_context();
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("JVM Android indisponível: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("Falha ao anexar thread à JVM: {error}"))?;
    let activity = unsafe { JObject::from_raw(context.context().cast()) };
    let result = handler(&mut env, &activity);
    // `activity` points at the global reference owned by `ndk-context`; dropping
    // the owning JObject would delete a reference we do not own.
    std::mem::forget(activity);
    result
}

fn java_string(env: &mut JNIEnv<'_>, value: JObject<'_>) -> Result<Option<String>, String> {
    if value.as_raw().is_null() {
        return Ok(None);
    }
    let value = JString::from(value);
    let value = env
        .get_string(&value)
        .map_err(|error| format!("Falha ao ler string Java: {error}"))?;
    Ok(Some(value.into()))
}

fn object_path(env: &mut JNIEnv<'_>, value: JObject<'_>) -> Result<PathBuf, String> {
    let path = env
        .call_method(&value, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .and_then(|value| value.l())
        .map_err(|error| format!("Falha ao resolver diretório Android: {error}"))?;
    java_string(env, path)?
        .map(PathBuf::from)
        .ok_or_else(|| "Diretório Android indisponível.".to_string())
}

fn call_dir(env: &mut JNIEnv<'_>, activity: &JObject<'_>, method: &str) -> Result<PathBuf, String> {
    let value = env
        .call_method(activity, method, "()Ljava/io/File;", &[])
        .and_then(|value| value.l())
        .map_err(|error| format!("Falha ao chamar {method}: {error}"))?;
    object_path(env, value)
}

/// Resolves the app data/cache and app-specific Downloads directories.
pub fn android_app_dirs() -> Result<AndroidAppDirs, String> {
    with_env(|env, activity| {
        let data_dir = call_dir(env, activity, "getFilesDir")?;
        let cache_dir = call_dir(env, activity, "getCacheDir")?;

        let downloads_name = env
            .new_string("Downloads")
            .map_err(|error| format!("Falha ao preparar diretório de downloads: {error}"))?;
        let external = env
            .call_method(
                activity,
                "getExternalFilesDir",
                "(Ljava/lang/String;)Ljava/io/File;",
                &[JValue::Object(&downloads_name)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao resolver downloads externos: {error}"))?;

        let download_dir = if external.as_raw().is_null() {
            data_dir.join("Downloads")
        } else {
            object_path(env, external)?
        };
        std::fs::create_dir_all(&download_dir)
            .map_err(|error| format!("Falha ao criar diretório de downloads: {error}"))?;

        Ok(AndroidAppDirs {
            data_dir: data_dir.clone(),
            local_data_dir: data_dir,
            cache_dir,
            download_dir,
        })
    })
}

fn extract_url(value: &str) -> Option<String> {
    value.split_whitespace().find_map(|part| {
        if !(part.starts_with("http://") || part.starts_with("https://")) {
            return None;
        }
        let trimmed = part.trim_end_matches([',', '.', ')', ']', '}', ';', '!', '?', '"', '\'']);
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

/// Reads the intent that launched the Activity and returns its shared URL.
pub fn take_initial_shared_url() -> Option<String> {
    with_env(|env, activity| {
        let intent = env
            .call_method(activity, "getIntent", "()Landroid/content/Intent;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao ler intent inicial: {error}"))?;
        if intent.as_raw().is_null() {
            return Ok(None);
        }

        let action = env
            .call_method(&intent, "getAction", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao ler ação do intent: {error}"))?;
        let action = java_string(env, action)?.unwrap_or_default();

        if action == "android.intent.action.SEND" || action == "android.intent.action.SEND_MULTIPLE"
        {
            let key = env
                .new_string("android.intent.extra.TEXT")
                .map_err(|error| error.to_string())?;
            let text = env
                .call_method(
                    &intent,
                    "getStringExtra",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&key)],
                )
                .and_then(|value| value.l())
                .map_err(|error| format!("Falha ao ler texto compartilhado: {error}"))?;
            if let Some(url) = java_string(env, text)?.as_deref().and_then(extract_url) {
                return Ok(Some(url));
            }
        }

        let data = env
            .call_method(&intent, "getDataString", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao ler URL do intent: {error}"))?;
        Ok(java_string(env, data)?.as_deref().and_then(extract_url))
    })
    .ok()
    .flatten()
}

/// Returns a URL received through a share intent before the UI registered its
/// handler.
pub fn take_pending_shared_url() -> Option<String> {
    PENDING_SHARED_URL.lock().ok()?.take()
}

/// Registers the URL share handler.
pub fn set_share_handler(handler: ShareHandler) {
    if let Ok(mut slot) = SHARE_HANDLER.lock() {
        *slot = Some(handler);
    }
}

/// Dispatches a URL received from `MainActivity.onNewIntent`.
pub fn dispatch_shared_url(url: String) {
    let Some(url) = extract_url(&url) else {
        return;
    };
    if let Ok(handler) = SHARE_HANDLER.lock() {
        if let Some(handler) = handler.as_ref() {
            handler(url);
            return;
        }
    }
    if let Ok(mut pending) = PENDING_SHARED_URL.lock() {
        *pending = Some(url);
    }
}

/// Starts the Android document picker and resolves with a cached local copy.
pub async fn pick_media_file() -> Option<PathBuf> {
    let (sender, receiver) = oneshot::channel();
    if let Ok(mut slot) = PICK_TX.lock() {
        *slot = Some(sender);
    }

    let started = with_env(|env, activity| {
        env.call_method(activity, "requestOpenMedia", "()V", &[])
            .map(|_| ())
            .map_err(|error| format!("Falha ao abrir seletor de arquivos: {error}"))
    });
    if let Err(error) = started {
        eprintln!("plasma: {error}");
        if let Ok(mut slot) = PICK_TX.lock() {
            slot.take();
        }
        return None;
    }

    receiver.await.ok().flatten()
}

/// Opens a file with an Android viewer.
pub fn open_path(path: &str) -> Result<(), String> {
    with_env(|env, activity| {
        let value = env
            .new_string(path)
            .map_err(|error| format!("Falha ao preparar caminho: {error}"))?;
        env.call_method(
            activity,
            "openPath",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&value)],
        )
        .map(|_| ())
        .map_err(|error| format!("Falha ao abrir arquivo: {error}"))
    })
}

/// Extracts a JPEG poster using `MediaMetadataRetriever`.
pub fn android_video_thumbnail(source_path: &str, output_path: &Path) -> Result<(), String> {
    let bytes = with_env(|env, _activity| {
        let retriever = env
            .new_object("android/media/MediaMetadataRetriever", "()V", &[])
            .map_err(|error| format!("Falha ao criar MediaMetadataRetriever: {error}"))?;
        let source = env
            .new_string(source_path)
            .map_err(|error| format!("Falha ao preparar caminho do vídeo: {error}"))?;
        env.call_method(
            &retriever,
            "setDataSource",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&source)],
        )
        .map_err(|error| format!("Falha ao abrir vídeo para thumbnail: {error}"))?;

        let mut bitmap = JObject::null();
        for time_us in [1_000_000_i64, 250_000, 0] {
            let frame = env
                .call_method(
                    &retriever,
                    "getFrameAtTime",
                    "(JI)Landroid/graphics/Bitmap;",
                    &[JValue::Long(time_us), JValue::Int(2)],
                )
                .and_then(|value| value.l())
                .map_err(|error| format!("Falha ao extrair frame do vídeo: {error}"))?;
            if !frame.as_raw().is_null() {
                bitmap = frame;
                break;
            }
        }

        if bitmap.as_raw().is_null() {
            let _ = env.call_method(&retriever, "release", "()V", &[]);
            return Err("Não foi possível extrair um frame do vídeo.".to_string());
        }

        let width = env
            .call_method(&bitmap, "getWidth", "()I", &[])
            .and_then(|value| value.i())
            .map_err(|error| format!("Falha ao ler largura do frame: {error}"))?;
        let height = env
            .call_method(&bitmap, "getHeight", "()I", &[])
            .and_then(|value| value.i())
            .map_err(|error| format!("Falha ao ler altura do frame: {error}"))?;

        let scaled = if width > 480 && height > 0 {
            let scaled_width = 480;
            let scaled_height = (height.saturating_mul(scaled_width) / width).max(1);
            env.call_static_method(
                "android/graphics/Bitmap",
                "createScaledBitmap",
                "(Landroid/graphics/Bitmap;IIZ)Landroid/graphics/Bitmap;",
                &[
                    JValue::Object(&bitmap),
                    JValue::Int(scaled_width),
                    JValue::Int(scaled_height),
                    JValue::Bool(1),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao redimensionar thumbnail: {error}"))?
        } else {
            JObject::null()
        };
        let final_bitmap = if scaled.as_raw().is_null() {
            &bitmap
        } else {
            &scaled
        };

        let format = env
            .get_static_field(
                "android/graphics/Bitmap$CompressFormat",
                "JPEG",
                "Landroid/graphics/Bitmap$CompressFormat;",
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao preparar formato JPEG: {error}"))?;
        let stream = env
            .new_object("java/io/ByteArrayOutputStream", "()V", &[])
            .map_err(|error| format!("Falha ao criar buffer de thumbnail: {error}"))?;
        env.call_method(
            final_bitmap,
            "compress",
            "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z",
            &[
                JValue::Object(&format),
                JValue::Int(85),
                JValue::Object(&stream),
            ],
        )
        .map_err(|error| format!("Falha ao comprimir thumbnail: {error}"))?;
        let bytes = env
            .call_method(&stream, "toByteArray", "()[B", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Falha ao ler thumbnail: {error}"))?;
        let bytes = JByteArray::from(bytes);
        let bytes = env
            .convert_byte_array(&bytes)
            .map_err(|error| format!("Falha ao copiar thumbnail: {error}"))?;

        let _ = env.call_method(&bitmap, "recycle", "()V", &[]);
        if !scaled.as_raw().is_null() {
            let _ = env.call_method(&scaled, "recycle", "()V", &[]);
        }
        let _ = env.call_method(&retriever, "release", "()V", &[]);
        Ok(bytes)
    })?;

    std::fs::write(output_path, bytes)
        .map_err(|error| format!("Falha ao salvar thumbnail Android: {error}"))
}

/// JNI callback invoked by `MainActivity` after a document picker result.
#[no_mangle]
pub extern "system" fn Java_dev_dioxus_main_MainActivity_nativeOnMediaPicked(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    path: JString<'_>,
) {
    let selected = if path.as_raw().is_null() {
        None
    } else {
        env.get_string(&path).ok().map(|value| {
            let value: String = value.into();
            PathBuf::from(value)
        })
    };
    if let Ok(mut slot) = PICK_TX.lock() {
        if let Some(sender) = slot.take() {
            let _ = sender.send(selected);
        }
    }
}

/// JNI callback invoked by `MainActivity` for a new share intent.
#[no_mangle]
pub extern "system" fn Java_dev_dioxus_main_MainActivity_nativeOnShareUrl(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    url: JString<'_>,
) {
    if url.as_raw().is_null() {
        return;
    }
    if let Ok(value) = env.get_string(&url) {
        let value: String = value.into();
        dispatch_shared_url(value);
    }
}
