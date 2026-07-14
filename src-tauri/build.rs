fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        tdlib_rs::build::build(None);
    }
    tauri_build::build()
}
