//! Native X/Twitter analyzer and downloader view.

use crate::downloads::{
    new_download_id, push_download, sanitize_filename, update_download, DownloadRecord,
};
use crate::state::{push_toast, AppServices, ToastKind, UiState};
use dioxus::prelude::*;
use plasma_twitter::{NativeFormatOption, NativeMediaInfo, NativeProfileInfo};
use std::path::Path;

#[component]
pub fn TwitterView() -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut profile_mode = use_signal(|| false);
    let mut url = use_signal(String::new);
    let mut cookies = use_signal(String::new);
    let mut cookies_loading = use_signal(|| true);
    let mut analyzing = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut tweet = use_signal(|| None::<NativeMediaInfo>);
    let mut profile = use_signal(|| None::<NativeProfileInfo>);

    let load_services = services.clone();
    use_future(move || {
        let load_services = load_services.clone();
        async move {
            match load_services.twitter.load_cookies().await {
                Ok(value) => cookies.set(value),
                Err(message) => error.set(Some(format!("Cookies do Twitter: {message}"))),
            }
            cookies_loading.set(false);
        }
    });

    let save_cookies = {
        let services = services.clone();
        move || {
            let value = cookies();
            let services = services.clone();
            spawn(async move {
                match services.twitter.save_cookies(value).await {
                    Ok(()) => push_toast(
                        ui.toasts,
                        "Cookies do Twitter salvos no arquivo local.",
                        ToastKind::Success,
                    ),
                    Err(error) => push_toast(
                        ui.toasts,
                        format!("Falha ao salvar cookies: {error}"),
                        ToastKind::Error,
                    ),
                }
            });
        }
    };

    let mut analyze = {
        let services = services.clone();
        move || {
            let raw_url = url().trim().to_string();
            if raw_url.is_empty() {
                error.set(Some(
                    "Informe a URL de um post ou perfil do X/Twitter.".to_string(),
                ));
                return;
            }
            let cookie_value = cookies().trim().to_string();
            let cookie_value = if cookie_value.is_empty() {
                None
            } else {
                Some(cookie_value)
            };
            analyzing.set(true);
            error.set(None);
            tweet.set(None);
            profile.set(None);
            let services = services.clone();
            let wants_profile = profile_mode() || looks_like_profile(&raw_url);
            profile_mode.set(wants_profile);
            spawn(async move {
                if wants_profile {
                    match services
                        .twitter
                        .analyze_profile(raw_url, cookie_value)
                        .await
                    {
                        Ok(info) => profile.set(Some(info)),
                        Err(message) => error.set(Some(message)),
                    }
                } else {
                    match services.twitter.analyze_tweet(raw_url, cookie_value).await {
                        Ok(info) => tweet.set(Some(info)),
                        Err(message) => error.set(Some(message)),
                    }
                }
                analyzing.set(false);
            });
        }
    };

    let analyze_enter = {
        let services = services.clone();
        move |event: KeyboardEvent| {
            if event.key() == Key::Enter {
                let raw_url = url().trim().to_string();
                if raw_url.is_empty() {
                    error.set(Some("Informe uma URL do X/Twitter.".to_string()));
                    return;
                }
                let cookie_value = cookies().trim().to_string();
                let cookie_value = if cookie_value.is_empty() {
                    None
                } else {
                    Some(cookie_value)
                };
                analyzing.set(true);
                error.set(None);
                tweet.set(None);
                profile.set(None);
                let services = services.clone();
                let wants_profile = profile_mode() || looks_like_profile(&raw_url);
                profile_mode.set(wants_profile);
                spawn(async move {
                    if wants_profile {
                        match services
                            .twitter
                            .analyze_profile(raw_url, cookie_value)
                            .await
                        {
                            Ok(info) => profile.set(Some(info)),
                            Err(message) => error.set(Some(message)),
                        }
                    } else {
                        match services.twitter.analyze_tweet(raw_url, cookie_value).await {
                            Ok(info) => tweet.set(Some(info)),
                            Err(message) => error.set(Some(message)),
                        }
                    }
                    analyzing.set(false);
                });
            }
        }
    };

    let current_mode = profile_mode();
    let current_cookies = cookies();
    let current_tweet = tweet();
    let current_profile = profile();

    rsx! {
        section { class: "twitter-view",
            div { class: "twitter-inner",
                header { class: "twitter-heading",
                    div {
                        h1 { "X / Twitter" }
                        p { class: "muted",
                            "Analisador nativo de posts e perfis, com download direto das \
                             variantes de vídeo e das mídias do perfil. Cookies são opcionais."
                        }
                    }
                    div { class: "segmented",
                        button {
                            class: if !current_mode { "active" } else { "" },
                            onclick: move |_| {
                                profile_mode.set(false);
                                error.set(None);
                            },
                            "Post"
                        }
                        button {
                            class: if current_mode { "active" } else { "" },
                            onclick: move |_| {
                                profile_mode.set(true);
                                error.set(None);
                            },
                            "Perfil"
                        }
                    }
                }

                section { class: "download-card twitter-search-card",
                    div { class: "twitter-search-row",
                        input {
                            class: "download-input",
                            r#type: "url",
                            placeholder: if current_mode {
                                "https://x.com/usuario"
                            } else {
                                "https://x.com/usuario/status/123..."
                            },
                            value: "{url}",
                            oninput: move |event| url.set(event.value()),
                            onkeydown: analyze_enter,
                        }
                        button {
                            class: "btn primary",
                            disabled: analyzing(),
                            onclick: move |_| analyze(),
                            if analyzing() { "Analisando..." } else { "Analisar" }
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "error-banner", "{message}" }
                    }
                    details { class: "twitter-cookies",
                        summary { "Cookies opcionais" }
                        p { class: "hint",
                            "Cole o valor do cookie do x.com para posts protegidos ou perfis \
                             com conteúdo sensível. O arquivo fica no diretório de dados do app."
                        }
                        textarea {
                            class: "twitter-cookies-input",
                            placeholder: "auth_token=...; ct0=...",
                            value: "{cookies}",
                            disabled: cookies_loading(),
                            oninput: move |event| cookies.set(event.value()),
                        }
                        button {
                            class: "btn small",
                            disabled: cookies_loading(),
                            onclick: move |_| save_cookies(),
                            "Salvar cookies"
                        }
                    }
                }

                if let Some(tweet) = current_tweet.as_ref() {
                    TweetCard { tweet: tweet.clone(), cookies: current_cookies.clone() }
                }
                if let Some(profile) = current_profile.as_ref() {
                    ProfileCard { profile: profile.clone(), cookies: current_cookies.clone() }
                }
                if !analyzing()
                    && current_tweet.is_none()
                    && current_profile.is_none()
                    && error().is_none()
                {
                    div { class: "empty-state twitter-empty",
                        div { class: "empty-card",
                            div { class: "empty-icon", "X" }
                            h2 { "Analise um post ou perfil" }
                            p {
                                "Cole uma URL do X/Twitter acima. O backend usa a API \
                                 pública, o GraphQL autenticado quando há cookies e o \
                                 fallback por syndication."
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TweetCard(tweet: NativeMediaInfo, cookies: String) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut formats = tweet.formats.video.clone();
    formats.extend(tweet.formats.audio.clone());
    let title = if tweet.title.trim().is_empty() {
        format!("Post de @{}", tweet.author)
    } else {
        tweet.title.clone()
    };
    let thumbnail = tweet.thumbnail_url.clone();

    rsx! {
        article { class: "twitter-result",
            div { class: "twitter-result-head",
                if let Some(thumbnail) = thumbnail {
                    img { class: "twitter-thumb", src: "{thumbnail}", alt: "" }
                } else {
                    div {
                        class: "twitter-thumb fallback",
                        style: "--thumb-hue: {tweet.thumb_hue}; --thumb-hue2: {tweet.thumb_hue2};",
                    }
                }
                div { class: "twitter-result-copy",
                    h2 { "{title}" }
                    p { class: "muted",
                        "@{tweet.author}"
                        if let Some(full) = tweet.author_full.as_ref() {
                            span { " · {full}" }
                        }
                        if !tweet.duration.trim().is_empty() {
                            span { " · {tweet.duration}" }
                        }
                    }
                    a {
                        class: "twitter-original",
                        href: "{tweet.original_url}",
                        target: "_blank",
                        "Abrir post original"
                    }
                }
            }
            if formats.is_empty() {
                p { class: "hint", "Nenhuma variante de vídeo/áudio foi encontrada neste post." }
            } else {
                div { class: "twitter-formats",
                    for option in formats {
                        {
                            let option_for_click = option.clone();
                            let tweet_for_click = tweet.clone();
                            let services_for_click = services.clone();
                            let cookies_for_click = cookies.clone();
                            rsx! {
                                div { class: "twitter-format", key: "{option.id}",
                                    div { class: "twitter-format-copy",
                                        strong {
                                            "{option.label}"
                                            if option.best.unwrap_or(false) {
                                                span { class: "pill", "melhor" }
                                            }
                                        }
                                        span { "{option.sub}" }
                                    }
                                    span { class: "muted", "{option.size}" }
                                    button {
                                        class: "btn small primary",
                                        disabled: option.url.is_none(),
                                        onclick: move |_| {
                                            if let Some(url) = option_for_click.url.clone() {
                                                download_tweet_format(
                                                    services_for_click.clone(),
                                                    ui,
                                                    &tweet_for_click,
                                                    &option_for_click,
                                                    url,
                                                    cookies_for_click.clone(),
                                                );
                                            }
                                        },
                                        "Baixar"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ProfileCard(profile: NativeProfileInfo, cookies: String) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let avatar = profile.avatar_url.clone();
    let media = profile
        .media_items
        .iter()
        .take(60)
        .cloned()
        .collect::<Vec<_>>();
    let shown_count = media.len();
    let username = profile.username.clone();
    let media_urls = profile.media_urls.clone();
    let original_url = profile.original_url.clone();
    let services_for_download = services.clone();
    let cookies_for_download = cookies.clone();

    rsx! {
        article { class: "twitter-result",
            div { class: "twitter-result-head",
                if let Some(avatar) = avatar {
                    img { class: "twitter-thumb avatar", src: "{avatar}", alt: "" }
                } else {
                    div { class: "twitter-thumb fallback avatar" }
                }
                div { class: "twitter-result-copy",
                    h2 {
                        if let Some(name) = profile.display_name.as_ref() {
                            "{name}"
                        } else {
                            "@{profile.username}"
                        }
                    }
                    p { class: "muted", "@{profile.username} · {profile.media_count} mídia(s)" }
                    a {
                        class: "twitter-original",
                        href: "{original_url}",
                        target: "_blank",
                        "Abrir perfil original"
                    }
                }
                div { class: "twitter-result-actions",
                    button {
                        class: "btn primary",
                        disabled: media_urls.is_empty(),
                        onclick: move |_| {
                            download_profile(
                                services_for_download.clone(),
                                ui,
                                &username,
                                &media_urls,
                                &original_url,
                                cookies_for_download.clone(),
                            );
                        },
                        "Baixar todas"
                    }
                }
            }
            if media.is_empty() {
                p { class: "hint", "Nenhuma mídia encontrada neste perfil." }
            } else {
                div { class: "twitter-media-grid",
                    for item in media {
                        div { class: "twitter-media-item", key: "{item.url}",
                            if let Some(thumbnail) = item.thumbnail_url.as_ref() {
                                img { src: "{thumbnail}", alt: "", loading: "lazy" }
                            } else if !item.is_video {
                                img { src: "{item.url}", alt: "", loading: "lazy" }
                            } else {
                                div { class: "thumb-placeholder", "▶" }
                            }
                            if item.is_video {
                                span { class: "thumb-play", "▶" }
                            }
                        }
                    }
                }
                if profile.media_count > shown_count {
                    p { class: "hint",
                        "Mostrando as primeiras {shown_count} de {profile.media_count} mídias. \
                         “Baixar todas” inclui o restante."
                    }
                }
            }
        }
    }
}

fn download_tweet_format(
    services: AppServices,
    ui: UiState,
    tweet: &NativeMediaInfo,
    option: &NativeFormatOption,
    url: String,
    cookies: String,
) {
    let id = format!(
        "twitter-{}-{}",
        tweet_slug(&tweet.original_url),
        new_download_id()
    );
    let extension = media_extension(&url);
    let name = format!(
        "twitter-{}-{}.{}",
        tweet_slug(&tweet.original_url),
        sanitize_filename(&option.id),
        extension
    );
    let destination = services.downloads.destination_for(&name);
    let file_name = Path::new(&destination)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&name)
        .to_string();
    let mut record =
        DownloadRecord::new_url(id.clone(), file_name.clone(), destination, url.clone());
    record.platform = Some("twitter".to_string());
    record.source_page_url = Some(tweet.original_url.clone());
    record.source_label = Some(format!("@{}", tweet.author));
    push_download(services.clone(), ui, record);
    let cookies = if cookies.trim().is_empty() {
        None
    } else {
        Some(cookies)
    };
    spawn(async move {
        let result = services
            .twitter
            .download_tweet(id.clone(), url, file_name, cookies)
            .await;
        finish_twitter_download(services, ui, id, result);
    });
}

fn download_profile(
    services: AppServices,
    ui: UiState,
    username: &str,
    media_urls: &[String],
    source_page_url: &str,
    cookies: String,
) {
    if media_urls.is_empty() {
        push_toast(ui.toasts, "Nenhuma mídia para baixar.", ToastKind::Info);
        return;
    }
    let id = format!(
        "twitter-profile-{}-{}",
        sanitize_filename(username),
        new_download_id()
    );
    let folder = services
        .twitter
        .download_dir()
        .join(format!("@{}", sanitize_filename(username)));
    let record = DownloadRecord {
        id: id.clone(),
        file_name: format!("@{} ({} mídias)", username, media_urls.len()),
        progress: 0.0,
        status: "downloading".to_string(),
        file_path: Some(folder.display().to_string()),
        file_size: None,
        source_url: Some(source_page_url.to_string()),
        source_page_url: Some(source_page_url.to_string()),
        platform: Some("twitter".to_string()),
        source_label: Some(format!("@{username}")),
        batch_id: Some(id.clone()),
        batch_title: Some(format!("Perfil @{username}")),
        batch_kind: Some("bulk".to_string()),
        batch_total: Some(media_urls.len() as i64),
        batch_downloaded: Some(0),
        created_at: Some(crate::state::now_millis()),
        can_cancel: Some(true),
        can_retry: Some(false),
        ..DownloadRecord::default()
    };
    push_download(services.clone(), ui, record);
    let urls = media_urls.to_vec();
    let username = username.to_string();
    let cookies = if cookies.trim().is_empty() {
        None
    } else {
        Some(cookies)
    };
    spawn(async move {
        let result = services
            .twitter
            .download_profile(id.clone(), username, urls, cookies)
            .await;
        finish_twitter_download(services, ui, id, result);
    });
}

fn finish_twitter_download(
    services: AppServices,
    ui: UiState,
    id: String,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            update_download(
                services,
                ui,
                &id,
                |item| {
                    item.progress = 100.0;
                    item.status = "completed".to_string();
                    item.error = None;
                    item.can_cancel = Some(false);
                    item.can_retry = Some(false);
                },
                true,
            );
        }
        Err(error) if error == plasma_download::DOWNLOAD_CANCELED_ERROR => {
            update_download(
                services,
                ui,
                &id,
                |item| {
                    item.status = "canceled".to_string();
                    item.error = None;
                    item.can_cancel = Some(false);
                    item.can_retry = Some(true);
                },
                true,
            );
        }
        Err(error) => {
            push_toast(
                ui.toasts,
                format!("Download do X falhou: {error}"),
                ToastKind::Error,
            );
            update_download(
                services,
                ui,
                &id,
                |item| {
                    item.status = "failed".to_string();
                    item.error = Some(error);
                    item.can_cancel = Some(false);
                    item.can_retry = Some(true);
                },
                true,
            );
        }
    }
}

fn looks_like_profile(input: &str) -> bool {
    let trimmed = input.trim();
    let handle = trimmed.trim_start_matches('@');
    if !handle.is_empty()
        && handle.len() <= 15
        && handle
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return true;
    }
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let path = if let Some((_, rest)) = without_query.split_once("://") {
        let Some((host, path)) = rest.split_once('/') else {
            return false;
        };
        let host = host.to_ascii_lowercase();
        if !matches!(
            host.as_str(),
            "x.com" | "www.x.com" | "twitter.com" | "www.twitter.com"
        ) {
            return false;
        }
        path
    } else {
        return false;
    };
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.len() == 1
        && parts[0].len() <= 15
        && parts[0]
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn media_extension(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "m4a" => "m4a",
        "mp3" => "mp3",
        "webm" => "webm",
        "mov" => "mov",
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        "gif" => "gif",
        _ => "mp4",
    }
}

fn tweet_slug(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let candidate = parts
        .windows(2)
        .find(|pair| pair[0] == "status")
        .map(|pair| pair[1])
        .or_else(|| parts.last().copied())
        .unwrap_or("twitter");
    sanitize_filename(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_media_extensions() {
        assert_eq!(
            media_extension("https://video.twimg.com/a.mp4?tag=1"),
            "mp4"
        );
        assert_eq!(
            media_extension("https://video.twimg.com/a.m4a?tag=1"),
            "m4a"
        );
        assert_eq!(media_extension("https://video.twimg.com/a"), "mp4");
    }

    #[test]
    fn derives_slug_from_status_url() {
        assert_eq!(tweet_slug("https://x.com/user/status/12345?ref=1"), "12345");
    }

    #[test]
    fn detects_profiles_and_posts() {
        assert!(looks_like_profile("@usuario"));
        assert!(looks_like_profile("usuario"));
        assert!(looks_like_profile("https://x.com/usuario"));
        assert!(!looks_like_profile("https://x.com/usuario/status/123"));
        assert!(!looks_like_profile("https://example.com/usuario"));
    }
}
