//! Inline SVG icons used by the Rust shell.
//!
//! Path data is based on Tabler Icons (<https://tabler.io/icons>), MIT
//! licensed. See `THIRD_PARTY_NOTICES.md` for the full license text. The shell
//! keeps a small, dependency-free subset so buttons and menus can use crisp
//! vector glyphs instead of emoji, and so the visuals stay consistent across
//! the whole app.

use dioxus::prelude::*;

/// Icons available to the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Reply,
    Forward,
    Copy,
    Download,
    CloudDownload,
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
    Paperclip,
    Send,
    BrandTelegram,
    ProgressDown,
    BrandTwitter,
    Settings,
    ChevronDown,
    ChevronRight,
}

impl IconName {
    /// SVG path data for the icon.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            IconName::Reply => &["M9 14l-4 -4l4 -4", "M5 10h11a4 4 0 1 1 0 8h-1"],
            IconName::Forward => &["M15 14l4 -4l-4 -4", "M19 10h-11a4 4 0 1 0 0 8h1"],
            IconName::Copy => &[
                "M7 9.667a2.667 2.667 0 0 1 2.667 -2.667h8.666a2.667 2.667 0 0 1 2.667 2.667v8.666a2.667 2.667 0 0 1 -2.667 2.667h-8.666a2.667 2.667 0 0 1 -2.667 -2.667l0 -8.666",
                "M4.012 16.737a2.005 2.005 0 0 1 -1.012 -1.737v-10c0 -1.1 .9 -2 2 -2h10c.75 0 1.158 .385 1.5 1",
            ],
            IconName::Download => &[
                "M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2 -2v-2",
                "M7 11l5 5l5 -5",
                "M12 4l0 12",
            ],
            IconName::CloudDownload => &[
                "M19 18a3.5 3.5 0 0 0 0 -7h-1a5 4.5 0 0 0 -11 -2a4.6 4.4 0 0 0 -2.1 8.4",
                "M12 13l0 9",
                "M9 19l3 3l3 -3",
            ],
            IconName::Save => &[
                "M6 4h10l4 4v10a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2",
                "M10 14a2 2 0 1 0 4 0a2 2 0 1 0 -4 0",
                "M14 4l0 4l-6 0l0 -4",
            ],
            IconName::Media => &[
                "M15 8h.01",
                "M3 6a3 3 0 0 1 3 -3h12a3 3 0 0 1 3 3v12a3 3 0 0 1 -3 3h-12a3 3 0 0 1 -3 -3v-12",
                "M3 16l5 -5c.928 -.893 2.072 -.893 3 0l5 5",
                "M14 14l1 -1c.928 -.893 2.072 -.893 3 0l3 3",
            ],
            IconName::Folder => &[
                "M5 4h4l3 3h7a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2",
            ],
            IconName::External => &[
                "M12 6h-6a2 2 0 0 0 -2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-6",
                "M11 13l9 -9",
                "M15 4h5v5",
            ],
            IconName::Trash => &[
                "M4 7l16 0",
                "M10 11l0 6",
                "M14 11l0 6",
                "M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2 -2l1 -12",
                "M9 7v-3a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v3",
            ],
            IconName::Refresh => &[
                "M20 11a8.1 8.1 0 0 0 -15.5 -2m-.5 -4v4h4",
                "M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4",
            ],
            IconName::Search => &[
                "M3 10a7 7 0 1 0 14 0a7 7 0 1 0 -14 0",
                "M21 21l-6 -6",
            ],
            IconName::Close => &["M18 6l-12 12", "M6 6l12 12"],
            IconName::Sparkles => &[
                "M16 18a2 2 0 0 1 2 2a2 2 0 0 1 2 -2a2 2 0 0 1 -2 -2a2 2 0 0 1 -2 2m0 -12a2 2 0 0 1 2 2a2 2 0 0 1 2 -2a2 2 0 0 1 -2 -2a2 2 0 0 1 -2 2m-7 12a6 6 0 0 1 6 -6a6 6 0 0 1 -6 -6a6 6 0 0 1 -6 6a6 6 0 0 1 6 6",
            ],
            IconName::Check => &["M5 12l5 5l10 -10"],
            IconName::More => &[
                "M4 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
                "M11 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
                "M18 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
            ],
            IconName::Eye => &[
                "M10 12a2 2 0 1 0 4 0a2 2 0 0 0 -4 0",
                "M21 12c-2.4 4 -5.4 6 -9 6c-3.6 0 -6.6 -2 -9 -6c2.4 -4 5.4 -6 9 -6c3.6 0 6.6 2 9 6",
            ],
            IconName::EyeOff => &[
                "M10.585 10.587a2 2 0 0 0 2.829 2.828",
                "M16.681 16.673a8.717 8.717 0 0 1 -4.681 1.327c-3.6 0 -6.6 -2 -9 -6c1.272 -2.12 2.712 -3.678 4.32 -4.674m2.86 -1.146a9.055 9.055 0 0 1 1.82 -.18c3.6 0 6.6 2 9 6c-.666 1.11 -1.379 2.067 -2.138 2.87",
                "M3 3l18 18",
            ],
            IconName::List => &[
                "M9 6l11 0",
                "M9 12l11 0",
                "M9 18l11 0",
                "M5 6l0 .01",
                "M5 12l0 .01",
                "M5 18l0 .01",
            ],
            IconName::Stop => &[
                "M5 7a2 2 0 0 1 2 -2h10a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-10a2 2 0 0 1 -2 -2l0 -10",
            ],
            IconName::Play => &["M7 4v16l13 -8l-13 -8"],
            IconName::Link => &[
                "M9 15l6 -6",
                "M11 6l.463 -.536a5 5 0 0 1 7.071 7.072l-.534 .464",
                "M13 18l-.397 .534a5.068 5.068 0 0 1 -7.127 0a4.972 4.972 0 0 1 0 -7.071l.524 -.463",
            ],
            IconName::Chat => &[
                "M8 9h8",
                "M8 13h6",
                "M18 4a3 3 0 0 1 3 3v8a3 3 0 0 1 -3 3h-5l-5 3v-3h-2a3 3 0 0 1 -3 -3v-8a3 3 0 0 1 3 -3h12",
            ],
            IconName::Smile => &[
                "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0",
                "M9 10l.01 0",
                "M15 10l.01 0",
                "M9.5 15a3.5 3.5 0 0 0 5 0",
            ],
            IconName::Info => &[
                "M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0",
                "M12 9h.01",
                "M11 12h1v4h1",
            ],
            IconName::Paperclip => &[
                "M15 7l-6.5 6.5a1.5 1.5 0 0 0 3 3l6.5 -6.5a3 3 0 0 0 -6 -6l-6.5 6.5a4.5 4.5 0 0 0 9 9l6.5 -6.5",
            ],
            IconName::Send => &[
                "M10 14l11 -11",
                "M21 3l-6.5 18a.55 .55 0 0 1 -1 0l-3.5 -7l-7 -3.5a.55 .55 0 0 1 0 -1l18 -6.5",
            ],
            IconName::BrandTelegram => &["M15 10l-4 4l6 6l4 -16l-18 7l4 2l2 6l3 -4"],
            IconName::ProgressDown => &[
                "M10 20.777a8.942 8.942 0 0 1 -2.48 -.969",
                "M14 3.223a9.003 9.003 0 0 1 0 17.554",
                "M4.579 17.093a8.961 8.961 0 0 1 -1.227 -2.592",
                "M3.124 10.5c.16 -.95 .468 -1.85 .9 -2.675l.169 -.305",
                "M6.907 4.579a8.954 8.954 0 0 1 3.093 -1.356",
                "M12 9v6",
                "M15 12l-3 3l-3 -3",
            ],
            IconName::BrandTwitter => &[
                "M22 4.01c-1 .49 -1.98 .689 -3 .99c-1.121 -1.265 -2.783 -1.335 -4.38 -.737s-2.643 2.06 -2.62 3.737v1c-3.245 .083 -6.135 -1.395 -8 -4c0 0 -4.182 7.433 4 11c-1.872 1.247 -3.739 2.088 -6 2c3.308 1.803 6.913 2.423 10.034 1.517c3.58 -1.04 6.522 -3.723 7.651 -7.742a13.84 13.84 0 0 0 .497 -3.753c0 -.249 1.51 -2.772 1.818 -4.013l0 .001",
            ],
            IconName::Settings => &[
                "M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 3.35a1.724 1.724 0 0 0 -1.066 2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 -2.924 1.756 -3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 -1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 .826 -3.31 2.37 -2.37c1 .608 2.296 .07 2.572 -1.065",
                "M9 12a3 3 0 1 0 6 0a3 3 0 0 0 -6 0",
            ],
            IconName::ChevronDown => &["M6 9l6 6l6 -6"],
            IconName::ChevronRight => &["M9 6l6 6l-6 6"],
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
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for path in name.paths() {
                path { d: "{path}" }
            }
        }
    }
}
