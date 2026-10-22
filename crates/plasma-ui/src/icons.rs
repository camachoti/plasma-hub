//! Inline SVG icons used by the Rust shell.
//!
//! The React app ships an icon set; the shell keeps a small, dependency free
//! equivalent so buttons and menus can use crisp vector glyphs instead of
//! emoji, and so the visuals stay consistent across the whole app.

use dioxus::prelude::*;

/// Icons available to the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Reply,
    Forward,
    Copy,
    Download,
    Save,
    Media,
    Folder,
    External,
    Trash,
    Refresh,
    Search,
    Close,
    Sparkles,
    Check,
    More,
    Eye,
    EyeOff,
    List,
    Stop,
    Play,
    Link,
    Chat,
    Smile,
    Info,
    ChevronDown,
    ChevronRight,
}

impl IconName {
    /// SVG path data for the icon.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            IconName::Reply => &["M9.5 14.5 4 9l5.5-5.5", "M4 9h9.5a6.5 6.5 0 0 1 6.5 6.5V19"],
            IconName::Forward => &[
                "M14.5 14.5 20 9l-5.5-5.5",
                "M20 9h-9.5A6.5 6.5 0 0 0 4 15.5V19",
            ],
            IconName::Copy => &[
                "M9.5 9.5h9.5V19H9.5z",
                "M5 14.5H4.5A1.5 1.5 0 0 1 3 13V4.5A1.5 1.5 0 0 1 4.5 3H13a1.5 1.5 0 0 1 1.5 1.5V5",
            ],
            IconName::Download => &["M12 3.5v11", "m7.5 10.5 4.5 4.5 4.5-4.5", "M4.5 20.5h15"],
            IconName::Save => &[
                "M4.5 4.5h11l4 4v11h-15z",
                "M8.5 4.5v5h7v-5",
                "M8.5 19.5v-5h7v5",
            ],
            IconName::Media => &[
                "M3.5 5.5h17v13h-17z",
                "m3.5 15 4.5-4.5 3.5 3.5 3-3 6 6",
                "M9 9.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z",
            ],
            IconName::Folder => &["M3.5 6.5h6l2 2.5h9v9.5h-17z"],
            IconName::External => &["M14 4.5h5.5V10", "M19.5 4.5 11 13", "M18 14v5.5H4.5V6H10"],
            IconName::Trash => &[
                "M4.5 7h15",
                "M9.5 7V4.5h5V7",
                "m6.5 7 1 12.5h9L17.5 7",
                "M10.5 10.5v6",
                "M13.5 10.5v6",
            ],
            IconName::Refresh => &["M20 12a8 8 0 1 1-2.4-5.7", "M20 4v5h-5"],
            IconName::Search => &[
                "M11 4.5a6.5 6.5 0 1 1 0 13 6.5 6.5 0 0 1 0-13Z",
                "m20 20-4.2-4.2",
            ],
            IconName::Close => &["m6.5 6.5 11 11", "m17.5 6.5-11 11"],
            IconName::Sparkles => &[
                "M12 3.5 13.6 8 18 9.5 13.6 11 12 15.5 10.4 11 6 9.5 10.4 8Z",
                "M18.5 14.5 19.2 16.4 21 17 19.2 17.6 18.5 19.5 17.8 17.6 16 17 17.8 16.4Z",
            ],
            IconName::Check => &["m4.5 12.5 5 5 10-11"],
            IconName::More => &["M11 5.5h2v2h-2z", "M11 11h2v2h-2z", "M11 16.5h2v2h-2z"],
            IconName::Eye => &[
                "M2.5 12S6 6.5 12 6.5 21.5 12 21.5 12 18 17.5 12 17.5 2.5 12 2.5 12Z",
                "M12 9.8a2.2 2.2 0 1 1 0 4.4 2.2 2.2 0 0 1 0-4.4Z",
            ],
            IconName::EyeOff => &[
                "m3.5 3.5 17 17",
                "M10.4 6.9a9.7 9.7 0 0 1 1.6-.1c6 0 9.5 5.2 9.5 5.2a17.6 17.6 0 0 1-2.9 3.3",
                "M6.5 7.6A16.6 16.6 0 0 0 2.5 12s3.5 5.2 9.5 5.2c1.1 0 2.2-.2 3.1-.5",
                "M9.9 9.9a2.2 2.2 0 0 0 3.1 3.1",
            ],
            IconName::List => &[
                "M8.5 6.5h12",
                "M8.5 12h12",
                "M8.5 17.5h12",
                "M4 6.5h.01",
                "M4 12h.01",
                "M4 17.5h.01",
            ],
            IconName::Stop => &["M6.5 6.5h11v11h-11z"],
            IconName::Play => &["m8 5 11 7-11 7z"],
            IconName::Link => &[
                "M10.5 13.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1",
                "M13.5 10.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1",
            ],
            IconName::Chat => &["M20.5 11.5a8 8 0 0 1-11.6 7.1L4 20l1.4-4.9a8 8 0 1 1 15.1-3.6Z"],
            IconName::Info => &[
                "M12 3.5a8.5 8.5 0 1 1 0 17 8.5 8.5 0 0 1 0-17Z",
                "M12 11v5.5",
                "M12 7.6h.01",
            ],
            IconName::Smile => &[
                "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17",
                "M9.2 10h.01",
                "M14.8 10h.01",
                "M8.5 14s1.3 2 3.5 2 3.5-2 3.5-2",
            ],
            IconName::ChevronDown => &["m6.5 9.5 5.5 5.5 5.5-5.5"],
            IconName::ChevronRight => &["m9.5 6.5 5.5 5.5-5.5 5.5"],
        }
    }
}

/// Renders one of the shell icons.
#[component]
pub fn Icon(name: IconName, size: u32, class: String) -> Element {
    rsx! {
        svg {
            class: "{class}",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.7",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for path in name.paths() {
                path { d: "{path}" }
            }
        }
    }
}
