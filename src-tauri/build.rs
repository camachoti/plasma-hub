fn main() {
    build_tdlib();
    tauri_build::build()
}

#[cfg(not(target_os = "android"))]
fn build_tdlib() {
    tdlib_rs::build::build(None);
}

#[cfg(target_os = "android")]
fn build_tdlib() {}
