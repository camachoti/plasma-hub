//! Embeds the Telegram API credentials used by the Rust shell.
//!
//! The Tauri frontend reads `VITE_API_ID`/`VITE_API_HASH` from the Vite env at
//! build time. The Rust shell mirrors that behaviour so both apps talk to the
//! same Telegram application without committing secrets. Runtime
//! `PLASMA_TELEGRAM_API_*` variables take precedence, which is convenient for
//! CI and release builds.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    for path in ["build.rs", ".env", "../../.env"] {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in [
        "VITE_API_ID",
        "VITE_API_HASH",
        "PLASMA_TELEGRAM_API_ID",
        "PLASMA_TELEGRAM_API_HASH",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    let api_id = lookup("PLASMA_TELEGRAM_API_ID")
        .or_else(|| lookup("VITE_API_ID"))
        .unwrap_or_default();
    let api_hash = lookup("PLASMA_TELEGRAM_API_HASH")
        .or_else(|| lookup("VITE_API_HASH"))
        .unwrap_or_default();

    println!("cargo:rustc-env=PLASMA_TELEGRAM_API_ID={api_id}");
    println!("cargo:rustc-env=PLASMA_TELEGRAM_API_HASH={api_hash}");

    emit_tdlib_rpath();
}

/// `tdlib-rs` downloads a shared `libtdjson` into its own build directory.
/// Cargo propagates the link search path to dependents but not the rpath, so a
/// direct `cargo run` cannot find the library. Add the downloaded directory to
/// the binary's rpath until packaging bundles the shared library.
fn emit_tdlib_rpath() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        return;
    }
    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    let Some(profile_dir) = PathBuf::from(out_dir).ancestors().nth(3).map(PathBuf::from) else {
        return;
    };
    let build_dir = profile_dir.join("build");
    let Ok(entries) = fs::read_dir(build_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("tdlib-rs-") {
            continue;
        }
        let lib_dir = entry.path().join("out").join("tdlib").join("lib");
        if lib_dir.join("libtdjson.so").exists() || lib_dir.join("libtdjson.so.1.8.61").exists() {
            println!(
                "cargo:rustc-link-arg=-Wl,-rpath,{}",
                lib_dir.to_string_lossy()
            );
        }
    }
}

fn lookup(key: &str) -> Option<String> {
    if let Ok(value) = env::var(key) {
        let value = clean(value);
        if !value.is_empty() {
            return Some(value);
        }
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    for relative in ["../../.env", ".env"] {
        let path = manifest_dir.join(relative);
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim() == key {
                let value = clean(value.to_string());
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }

    None
}

fn clean(value: String) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value);
    value.trim().to_string()
}
