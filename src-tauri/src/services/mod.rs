#[cfg(not(target_os = "android"))]
pub mod telegram;

#[cfg(target_os = "android")]
pub mod telegram_android;

pub mod twitter;

pub mod youtube;
